#[compute]
// Collision-only local-gravity pipeline.
#version 450

layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;

struct Particle {
    vec4 position_mass;
    vec4 velocity_charge;
    vec4 color_radius;
};

layout(set = 0, binding = 0, std430) restrict buffer InputParticles {
    Particle input_particles[];
};
layout(set = 0, binding = 1, std430) restrict buffer OutputParticles {
    Particle output_particles[];
};
layout(set = 0, binding = 2, std430) restrict buffer CellCounts {
    uint cell_counts[];
};
layout(set = 0, binding = 3, std430) restrict buffer CellIndices {
    uint cell_indices[];
};
layout(set = 0, binding = 4, std430) restrict buffer MergeCounts {
    uint merge_counts[];
};
layout(set = 0, binding = 5, std430) restrict buffer MergeFlags {
    uint merge_flags[];
};
layout(set = 0, binding = 6, std430) restrict buffer InputCount {
    uint input_count[];
};
layout(set = 0, binding = 7, std430) restrict buffer OutputCount {
    uint output_count[];
};

layout(push_constant, std430) uniform Params {
    vec4 simulation; // dt, gravity, force scale, boundary radius
    vec4 grid; // dimension, cell size, grid origin, max entries per cell
    vec4 merge; // merge radius, merge speed, softening, collapse strength
    vec4 lifecycle; // expansion strength, expansion duration, time, pass
} params;

uint grid_dimension() { return uint(params.grid.x); }
uint grid_capacity() { return uint(params.grid.w); }
uint grid_cell_count() { return grid_dimension() * grid_dimension() * grid_dimension(); }

uint cell_index_for_position(vec3 position) {
    int dimension = int(grid_dimension());
    vec3 local_position = (position + vec3(params.grid.z)) / params.grid.y;
    ivec3 cell = ivec3(floor(local_position));
    cell = clamp(cell, ivec3(0), ivec3(dimension - 1));
    return uint(cell.x + cell.y * dimension + cell.z * dimension * dimension);
}

ivec3 cell_coordinates(vec3 position) {
    int dimension = int(grid_dimension());
    vec3 local_position = (position + vec3(params.grid.z)) / params.grid.y;
    return clamp(ivec3(floor(local_position)), ivec3(0), ivec3(dimension - 1));
}

uint flatten_cell(ivec3 cell) {
    int dimension = int(grid_dimension());
    return uint(cell.x + cell.y * dimension + cell.z * dimension * dimension);
}

void clear_pass(uint index, uint reset_merge_state) {
    uint cell_count = grid_cell_count();
    if (index < cell_count) {
        cell_counts[index] = 0u;
    }
    uint particle_capacity = input_count[0];
    if (reset_merge_state != 0u && index < particle_capacity) {
        merge_counts[index] = 0u;
        merge_flags[index] = 0u;
    }
    if (reset_merge_state != 0u && index == 0u) {
        output_count[0] = 0u;
    }
}

void build_hash(uint index) {
    uint count = input_count[0];
    if (index >= count) {
        return;
    }
    Particle particle = input_particles[index];
    uint cell = cell_index_for_position(particle.position_mass.xyz);
    uint slot = atomicAdd(cell_counts[cell], 1u);
    if (slot < grid_capacity()) {
        cell_indices[cell * grid_capacity() + slot] = index;
    }
}

void integrate_local_gravity(uint index) {
    uint count = input_count[0];
    if (index >= count) {
        return;
    }
    Particle particle = input_particles[index];
    vec3 position = particle.position_mass.xyz;
    vec3 velocity = particle.velocity_charge.xyz;
    ivec3 base_cell = cell_coordinates(position);
    int dimension = int(grid_dimension());
    float softening = max(params.merge.z, 0.05);
    vec3 acceleration = vec3(0.0);

    // Barnes-Hut-style local field: the uniform grid limits interactions to
    // nearby cells, which preserves cluster formation without an O(N^2) pass.
    for (int z = 0; z <= 0; z++) {
        for (int y = 0; y <= 0; y++) {
            for (int x = 0; x <= 0; x++) {
                ivec3 neighbor = base_cell + ivec3(x, y, z);
                if (any(lessThan(neighbor, ivec3(0))) || any(greaterThanEqual(neighbor, ivec3(dimension)))) {
                    continue;
                }
                uint cell = flatten_cell(neighbor);
                uint cell_count = min(cell_counts[cell], grid_capacity());
                uint sample_count = min(cell_count, 8u);
                for (uint sample = 0u; sample < sample_count; sample++) {
                    uint slot = (index * 13u + sample * 7u) % cell_count;
                    uint candidate = cell_indices[cell * grid_capacity() + slot];
                    if (candidate >= count || candidate == index) {
                        continue;
                    }
                    vec3 offset = input_particles[candidate].position_mass.xyz - position;
                    float distance_squared = dot(offset, offset) + softening * softening;
                    float inverse_distance = inversesqrt(distance_squared);
                    float inverse_distance_cubed = inverse_distance * inverse_distance * inverse_distance;
                    acceleration += offset * (params.simulation.y * params.simulation.z * input_particles[candidate].position_mass.w * inverse_distance_cubed);
                }
            }
        }
    }

    // The expansion is an initial event only. There is no artificial global
    // inward force after it, so separated bodies remain separated unless local
    // gravity brings them back together.
    float age = params.lifecycle.z;
    float expansion_fade = max(1.0 - age / max(params.lifecycle.y, 0.001), 0.0);
    acceleration += normalize(position) * params.lifecycle.x * expansion_fade * 0.15;
    acceleration = clamp(acceleration, vec3(-100.0), vec3(100.0));
    velocity += acceleration * params.simulation.x;
    velocity *= exp(-0.008 * params.simulation.x);
    position += velocity * params.simulation.x;
    if (length(position) > params.simulation.w) {
        position = normalize(position) * params.simulation.w;
        velocity -= 1.8 * dot(velocity, normalize(position)) * normalize(position);
    }
    particle.position_mass.xyz = position;
    particle.velocity_charge.xyz = velocity;
    input_particles[index] = particle;
}

void merge_pass(uint index) {
    uint count = input_count[0];
    if (index >= count || merge_flags[index] != 0u || params.lifecycle.z < params.merge.w) {
        return;
    }
    Particle particle = input_particles[index];
    ivec3 base_cell = cell_coordinates(particle.position_mass.xyz);
    float best_distance = 1000000.0;
    uint best = count;
    int dimension = int(grid_dimension());
    for (int z = 0; z <= 0; z++) {
        for (int y = 0; y <= 0; y++) {
            for (int x = 0; x <= 0; x++) {
                ivec3 neighbor = base_cell + ivec3(x, y, z);
                if (any(lessThan(neighbor, ivec3(0))) || any(greaterThanEqual(neighbor, ivec3(dimension)))) {
                    continue;
                }
                uint cell = flatten_cell(neighbor);
                uint cell_count = min(cell_counts[cell], grid_capacity());
                for (uint slot = 0u; slot < cell_count; slot++) {
                    uint candidate = cell_indices[cell * grid_capacity() + slot];
                    if (candidate >= index || candidate >= count || merge_flags[candidate] != 0u) {
                        continue;
                    }
                    vec3 offset = input_particles[candidate].position_mass.xyz - particle.position_mass.xyz;
                    float distance = dot(offset, offset);
                    vec3 relative_velocity = input_particles[candidate].velocity_charge.xyz - particle.velocity_charge.xyz;
                    float approach_speed = dot(offset, relative_velocity);
                    float collision_radius = particle.color_radius.w + input_particles[candidate].color_radius.w;
                    if (distance < collision_radius * collision_radius && approach_speed < 0.0 && length(relative_velocity) < params.merge.y) {
                        best_distance = distance;
                        best = candidate;
                    }
                }
            }
        }
    }
    if (best < count && atomicExchange(merge_flags[index], 1u) == 0u) {
        uint accepted = atomicAdd(merge_counts[best], 1u);
        if (accepted >= 1u) {
            atomicAdd(merge_counts[best], 0xffffffffu);
            atomicExchange(merge_flags[index], 0u);
        } else {
        }
    }
}

void compact_pass(uint index) {
    uint count = input_count[0];
    if (index >= count || merge_flags[index] != 0u) {
        return;
    }
    Particle particle = input_particles[index];
    particle.position_mass.w += float(merge_counts[index]);
    particle.color_radius.w = min(pow(particle.position_mass.w, 0.3333333) * 0.15, 2.0);
    uint destination = atomicAdd(output_count[0], 1u);
    output_particles[destination] = particle;
}

void main() {
    uint index = gl_GlobalInvocationID.x;
    uint pass = uint(params.lifecycle.w);
    if (pass == 0u) {
        clear_pass(index, 1u);
    } else if (pass == 1u) {
        build_hash(index);
    } else if (pass == 2u) {
        integrate_local_gravity(index);
    } else if (pass == 3u) {
        clear_pass(index, 0u);
    } else if (pass == 4u) {
        build_hash(index);
    } else if (pass == 5u) {
        merge_pass(index);
    } else {
        compact_pass(index);
    }
}
