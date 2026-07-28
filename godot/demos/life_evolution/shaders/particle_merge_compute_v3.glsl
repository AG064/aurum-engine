#[compute]
#version 450

layout(local_size_x = 256, local_size_y = 1, local_size_z = 1) in;

struct Particle {
    vec4 position_mass;
    vec4 velocity_charge;
    vec4 color_radius;
    vec4 chemistry;
};

layout(set = 0, binding = 0, std430) buffer InputParticles { Particle input_particles[]; };
layout(set = 0, binding = 1, std430) buffer OutputParticles { Particle output_particles[]; };
layout(set = 0, binding = 2, std430) buffer CellCounts { uint cell_counts[]; };
layout(set = 0, binding = 3, std430) buffer CellIndices { uint cell_indices[]; };
layout(set = 0, binding = 4, std430) buffer MergeCounts { uint merge_counts[]; };
layout(set = 0, binding = 5, std430) buffer MergeFlags { uint merge_flags[]; };
layout(set = 0, binding = 6, std430) buffer MergePartners { uint merge_partners[]; };
layout(set = 0, binding = 7, std430) buffer InputCount { uint input_count[]; };
layout(set = 0, binding = 8, std430) buffer OutputCount { uint output_count[]; };
layout(set = 0, binding = 9, std430) buffer RenderInstances { vec4 render_instances[]; };
layout(set = 0, binding = 10, std430) buffer RenderCount { uint render_count[]; };
layout(set = 0, binding = 11, std430) buffer Diagnostics { uint diagnostics[]; };

layout(push_constant, std430) uniform Params {
    vec4 simulation;
    vec4 grid;
    vec4 merge;
    vec4 lifecycle;
    vec4 event;
} params;

uint dimension() { return uint(params.grid.x); }
uint cell_capacity() { return uint(params.grid.w); }
uint cell_total() { return dimension() * dimension() * dimension(); }
uint species_of(Particle particle) { return uint(round(particle.chemistry.x)); }
uint level_of(Particle particle) { return uint(round(particle.chemistry.y)); }
uint constituents_of(Particle particle) { return max(uint(round(particle.chemistry.w)), 1u); }

uint cell_for(vec3 position) {
    int d = int(dimension());
    vec3 local_position = (position + vec3(params.grid.z)) / params.grid.y;
    ivec3 cell = clamp(ivec3(floor(local_position)), ivec3(0), ivec3(d - 1));
    return uint(cell.x + cell.y * d + cell.z * d * d);
}

vec3 species_color(uint species, uint level) {
    if (species == 1u) return vec3(0.95, 0.18, 0.08);
    if (species == 2u) return vec3(0.72, 0.72, 0.78);
    if (species == 3u) return vec3(0.12, 0.72, 1.0);
    if (level == 1u) return vec3(1.0, 0.38, 0.08);
    if (level == 2u) return vec3(1.0, 0.72, 0.12);
    if (level == 3u) return vec3(0.12, 0.9, 0.52);
    return vec3(0.72, 0.22, 1.0);
}

bool can_react(Particle first, Particle second) {
    uint first_level = level_of(first);
    uint second_level = level_of(second);
    uint first_species = species_of(first);
    uint second_species = species_of(second);
    if (first_level == 0u && second_level == 0u) {
        bool proton_neutron = (first_species == 1u && second_species == 2u) || (first_species == 2u && second_species == 1u);
        bool proton_electron = (first_species == 1u && second_species == 3u) || (first_species == 3u && second_species == 1u);
        return proton_neutron || proton_electron;
    }
    if ((first_level == 1u && second_level == 0u) || (first_level == 0u && second_level == 1u)) {
        Particle nucleus = first_level == 1u ? first : second;
        Particle free_particle = first_level == 0u ? first : second;
        bool is_electron = species_of(free_particle) == 3u;
        bool is_nucleon = species_of(free_particle) == 1u || species_of(free_particle) == 2u;
        return nucleus.chemistry.z > 0.0 && (is_electron || is_nucleon);
    }
    if (first_level == 1u && second_level == 1u) {
        return first.chemistry.z > 0.0 && second.chemistry.z > 0.0;
    }
    if (first_level == 2u && second_level == 2u) {
        return first.chemistry.z > 0.0 && second.chemistry.z > 0.0 && abs(first.velocity_charge.w + second.velocity_charge.w) < 2.0;
    }
    if ((first_level == 3u && second_level == 2u) || (first_level == 2u && second_level == 3u)) {
        return first.chemistry.z > 0.0 && second.chemistry.z > 0.0;
    }
    if (first_level == 3u && second_level == 3u) {
        return first.chemistry.z > 0.0 && second.chemistry.z > 0.0;
    }
    return false;
}

void combine_chemistry(inout Particle product, Particle other) {
    uint first_level = level_of(product);
    uint second_level = level_of(other);
    uint first_species = species_of(product);
    uint second_species = species_of(other);
    uint total_constituents = min(constituents_of(product) + constituents_of(other), 8u);
    uint result_level = 0u;
    uint result_species = 1u;
    float free_slots = 1.0;
    if (first_level == 0u && second_level == 0u) {
        bool has_proton = first_species == 1u || second_species == 1u;
        bool has_neutron = first_species == 2u || second_species == 2u;
        bool has_electron = first_species == 3u || second_species == 3u;
        if (has_proton && has_neutron && !has_electron) {
            result_level = 1u;
            result_species = 11u;
            free_slots = 2.0;
        } else {
            result_level = 2u;
            result_species = 10u;
            free_slots = 1.0;
        }
    } else if (first_level == 1u && second_level == 0u) {
        if (second_species == 3u) {
            result_level = 2u;
            result_species = first_species == 12u ? 20u : 10u;
            free_slots = 1.0;
        } else {
            result_level = 1u;
            result_species = first_species == 11u && constituents_of(product) + constituents_of(other) >= 4u ? 12u : 11u;
            free_slots = max(product.chemistry.z - 1.0, 0.0);
        }
    } else if (first_level == 0u && second_level == 1u) {
        if (first_species == 3u) {
            result_level = 2u;
            result_species = second_species == 12u ? 20u : 10u;
            free_slots = 1.0;
        } else {
            result_level = 1u;
            result_species = second_species == 11u && constituents_of(product) + constituents_of(other) >= 4u ? 12u : 11u;
            free_slots = max(other.chemistry.z - 1.0, 0.0);
        }
    } else if (first_level == 1u && second_level == 1u) {
        result_level = 1u;
        result_species = constituents_of(product) + constituents_of(other) >= 4u ? 12u : 11u;
        free_slots = max(product.chemistry.z + other.chemistry.z - 2.0, 0.0);
    } else if (first_level == 1u || second_level == 1u) {
        result_level = 2u;
        result_species = first_species == 12u || second_species == 12u ? 20u : 10u;
        free_slots = 1.0;
    } else if (first_level == 2u && second_level == 2u) {
        result_level = 3u;
        result_species = 100u;
        free_slots = 2.0;
    } else {
        result_level = 4u;
        result_species = 200u;
        free_slots = 4.0;
    }
    product.chemistry.x = float(result_species);
    product.chemistry.y = float(result_level);
    product.chemistry.z = free_slots;
    product.chemistry.w = float(total_constituents);
    product.color_radius.xyz = species_color(result_species, result_level);
}

void clear_pass(uint index, uint reset_state) {
    if (index < cell_total()) cell_counts[index] = 0u;
    uint count = input_count[0];
    if (reset_state != 0u && index < count) {
        merge_counts[index] = 0u;
        merge_flags[index] = 0u;
        merge_partners[index] = count;
    }
    if (reset_state != 0u && index == 0u) {
        output_count[0] = 0u;
        for (uint slot = 0u; slot < 13u; slot++) diagnostics[slot] = 0u;
    }
}

void hash_pass(uint index) {
    uint count = input_count[0];
    if (index >= count) return;
    uint cell = cell_for(input_particles[index].position_mass.xyz);
    uint slot = atomicAdd(cell_counts[cell], 1u);
    if (slot < cell_capacity()) cell_indices[cell * cell_capacity() + slot] = index;
}

void integrate_pass(uint index) {
    uint count = input_count[0];
    if (index >= count) return;
    Particle particle = input_particles[index];
    vec3 position = particle.position_mass.xyz;
    vec3 velocity = particle.velocity_charge.xyz;
    uint cell = cell_for(position);
    uint occupants = min(cell_counts[cell], cell_capacity());
    float softening = max(params.merge.z, 0.05);
    vec3 acceleration = vec3(0.0);
    uint sample_count = min(occupants, 8u);
    for (uint sample_index = 0u; sample_index < sample_count; sample_index++) {
        uint candidate = cell_indices[cell * cell_capacity() + sample_index];
        if (candidate == index || candidate >= count) continue;
        vec3 offset = input_particles[candidate].position_mass.xyz - position;
        float distance_squared = dot(offset, offset) + softening * softening;
        float inverse_distance = inversesqrt(distance_squared);
        acceleration += offset * (params.simulation.y * params.simulation.z * input_particles[candidate].position_mass.w * inverse_distance * inverse_distance * inverse_distance);
    }
    float expansion_fade = max(1.0 - params.lifecycle.z / max(params.lifecycle.y, 0.001), 0.0);
    float distance_from_origin = max(length(position), 0.001);
    acceleration += position / distance_from_origin * params.lifecycle.x * expansion_fade * 0.15;
    if (params.lifecycle.z >= params.lifecycle.y) {
        // The local hash is intentionally capped for performance. Once the
        // cloud becomes sparse across cells, add a far-field self-gravity
        // approximation so bodies continue to respond instead of coasting
        // indefinitely with only velocity damping.
        float total_mass_estimate = max(float(count), 1.0);
        float radial_distance_squared = dot(position, position) + softening * softening;
        float radial_inverse_distance = inversesqrt(radial_distance_squared);
        acceleration -= position * (params.simulation.y * params.simulation.z * total_mass_estimate * radial_inverse_distance * radial_inverse_distance * radial_inverse_distance);
    }
    acceleration = clamp(acceleration, vec3(-100.0), vec3(100.0));
    uint acceleration_scaled = uint(clamp(length(acceleration) * 1000.0, 0.0, 4000000.0));
    atomicAdd(diagnostics[8], acceleration_scaled);
    atomicMax(diagnostics[9], acceleration_scaled);
    uint force_scaled = uint(clamp(length(acceleration) * particle.position_mass.w * 1000.0, 0.0, 4000000.0));
    atomicAdd(diagnostics[10], force_scaled);
    atomicMax(diagnostics[11], force_scaled);
    if (acceleration_scaled > 1u) atomicAdd(diagnostics[12], 1u);
    velocity += acceleration * params.simulation.x;
    velocity *= exp(-0.008 * params.simulation.x);
    position += velocity * params.simulation.x;
    float boundary = params.simulation.w;
    float distance_after = length(position);
    if (distance_after > boundary) {
        vec3 normal = position / distance_after;
        position = normal * boundary;
        velocity -= 1.8 * dot(velocity, normal) * normal;
    }
    particle.position_mass.xyz = position;
    particle.velocity_charge.xyz = velocity;
    input_particles[index] = particle;
}

void merge_pass(uint index) {
    uint count = input_count[0];
    if (index >= count || merge_flags[index] != 0u || params.lifecycle.z < params.merge.w) return;
    Particle particle = input_particles[index];
    uint cell = cell_for(particle.position_mass.xyz);
    uint occupants = min(cell_counts[cell], cell_capacity());
    uint best = count;
    float best_distance = 1000000.0;
    for (uint slot = 0u; slot < occupants; slot++) {
        uint candidate = cell_indices[cell * cell_capacity() + slot];
        if (candidate >= index || candidate >= count || merge_flags[candidate] != 0u) continue;
        Particle other = input_particles[candidate];
        if (!can_react(particle, other)) continue;
        vec3 offset = other.position_mass.xyz - particle.position_mass.xyz;
        float distance_squared = dot(offset, offset);
        vec3 relative_velocity = other.velocity_charge.xyz - particle.velocity_charge.xyz;
        float collision_radius = max(particle.color_radius.w + other.color_radius.w, params.merge.x);
        if (distance_squared < collision_radius * collision_radius && dot(offset, relative_velocity) < 0.0 && length(relative_velocity) < params.merge.y && distance_squared < best_distance) {
            best = candidate;
            best_distance = distance_squared;
        }
    }
    if (best < count && atomicExchange(merge_flags[index], 1u) == 0u) {
        uint accepted = atomicAdd(merge_counts[best], 1u);
        if (accepted < cell_capacity()) {
            merge_partners[index] = best;
        } else {
            atomicAdd(merge_counts[best], 0xffffffffu);
            atomicExchange(merge_flags[index], 0u);
        }
    }
}

void compact_pass(uint index) {
    uint count = input_count[0];
    if (index >= count || merge_flags[index] != 0u) return;
    Particle particle = input_particles[index];
    float total_mass = particle.position_mass.w;
    vec3 total_position = particle.position_mass.xyz * total_mass;
    vec3 total_momentum = particle.velocity_charge.xyz * total_mass;
    uint cell = cell_for(particle.position_mass.xyz);
    uint occupants = min(cell_counts[cell], cell_capacity());
    for (uint slot = 0u; slot < occupants; slot++) {
        uint candidate = cell_indices[cell * cell_capacity() + slot];
        if (candidate >= count || merge_flags[candidate] == 0u || merge_partners[candidate] != index) continue;
        float candidate_mass = input_particles[candidate].position_mass.w;
        total_mass += candidate_mass;
        total_position += input_particles[candidate].position_mass.xyz * candidate_mass;
        total_momentum += input_particles[candidate].velocity_charge.xyz * candidate_mass;
    }
    particle.position_mass.xyz = total_position / max(total_mass, 0.0001);
    particle.position_mass.w = total_mass;
    particle.velocity_charge.xyz = total_momentum / max(total_mass, 0.0001);
    if (merge_counts[index] > 0u) {
        for (uint slot = 0u; slot < occupants; slot++) {
            uint candidate = cell_indices[cell * cell_capacity() + slot];
            if (candidate < count && merge_flags[candidate] != 0u && merge_partners[candidate] == index) {
                combine_chemistry(particle, input_particles[candidate]);
                break;
            }
        }
    }
    particle.color_radius.w = min(pow(total_mass, 0.3333333) * 0.35, 6.0);
    uint destination = atomicAdd(output_count[0], 1u);
    output_particles[destination] = particle;
}

void render_clear_pass(uint index) {
    if (index == 0u) {
        render_count[0] = 0u;
        diagnostics[0] = output_count[0];
        diagnostics[1] = 0u;
        diagnostics[2] = 0u;
        diagnostics[3] = 0u;
    }
    uint base = index * 4u;
    // Clear the complete transform and color record. Clearing alpha alone is
    // insufficient when the material is opaque and leaves stale afterimages.
    render_instances[base + 0u] = vec4(0.0);
    render_instances[base + 1u] = vec4(0.0);
    render_instances[base + 2u] = vec4(0.0);
    render_instances[base + 3u] = vec4(0.0);
}

void render_pass(uint index) {
    uint active_count_render = output_count[0];
    if (index >= active_count_render) return;
    Particle particle = output_particles[index];
    uint visual_count = min(constituents_of(particle), 8u);
    uint level = level_of(particle);
    if (level == 2u) atomicAdd(diagnostics[1], 1u);
    else if (level == 3u) atomicAdd(diagnostics[2], 1u);
    else if (level >= 4u) atomicAdd(diagnostics[3], 1u);
    uint speed_scaled = uint(clamp(length(particle.velocity_charge.xyz) * 1000.0, 0.0, 4000000.0));
    uint radius_scaled = uint(clamp(length(particle.position_mass.xyz) * 1000.0, 0.0, 4000000.0));
    uint mass_scaled = uint(clamp(particle.position_mass.w * 1000.0, 0.0, 4000000.0));
    atomicAdd(diagnostics[4], speed_scaled);
    atomicMax(diagnostics[5], speed_scaled);
    atomicAdd(diagnostics[6], radius_scaled);
    atomicAdd(diagnostics[7], mass_scaled);
    uint destination = atomicAdd(render_count[0], visual_count);
    uint species = species_of(particle);
    float body_scale = particle.color_radius.w;
    vec3 position = particle.position_mass.xyz;
    vec3 axis = normalize(vec3(0.37 + float(index % 7u) * 0.11, 0.71, 0.29));
    for (uint component = 0u; component < 8u; component++) {
        if (component >= visual_count) break;
        float angle = float(component) * 6.2831853 / max(float(visual_count), 1.0) + params.lifecycle.z * 0.35;
        float orbit = body_scale * (level == 0u ? 0.0 : 0.8 + 0.15 * float(component % 3u));
        vec3 offset = vec3(cos(angle) * orbit, sin(angle) * orbit * 0.65, sin(angle * 1.7) * orbit * 0.35);
        if (level == 2u && component == 0u) offset = vec3(0.0);
        if (level == 1u) offset *= 0.6;
        vec3 component_position = position + offset;
        float component_scale = max(body_scale * (level >= 3u ? 0.24 : 0.32), 0.035);
        if (level == 0u) component_scale = max(body_scale * 0.8, 0.035);
        if (level == 2u && component == 0u) component_scale *= 1.25;
        uint base = (destination + component) * 4u;
        vec3 color = species_color(species, level);
        if (level == 2u && component > 0u) color = vec3(0.12, 0.72, 1.0);
        render_instances[base + 0u] = vec4(component_scale, 0.0, 0.0, component_position.x);
        render_instances[base + 1u] = vec4(0.0, component_scale, 0.0, component_position.y);
        render_instances[base + 2u] = vec4(0.0, 0.0, component_scale, component_position.z);
        render_instances[base + 3u] = vec4(color, 1.0);
    }
}

vec3 emission_direction(uint index) {
    float seed = float(index) + params.lifecycle.z * 17.0;
    float u = fract(sin(seed * 12.9898) * 43758.5453);
    float v = fract(sin(seed * 78.233) * 24634.6345);
    float z = 1.0 - 2.0 * u;
    float ring = sqrt(max(1.0 - z * z, 0.0));
    float angle = v * 6.2831853;
    return vec3(cos(angle) * ring, z, sin(angle) * ring);
}

void emit_pass(uint index) {
    uint emission_count = uint(params.event.y);
    if (params.event.x < 0.5 || index >= emission_count) return;
    uint base = output_count[0];
    uint destination = base + index;
    if (destination >= uint(params.event.z)) return;
    vec3 direction = emission_direction(index);
    float radius = 2.0 + fract(sin(float(index) * 91.17) * 43758.5453) * 3.0;
    float speed = max(params.lifecycle.x * 2.0, 1.0) + fract(sin(float(index) * 31.71) * 43758.5453) * 2.0;
    Particle particle;
    particle.position_mass = vec4(direction * radius, 1.0);
    particle.velocity_charge = vec4(direction * speed, 0.0);
    particle.color_radius = vec4(0.35, 0.65, 1.0, 0.35);
    particle.chemistry = vec4(1.0, 0.0, 1.0, 1.0);
    uint species_roll = index % 10u;
    if (species_roll < 4u) {
        particle.position_mass.w = 1.0;
        particle.velocity_charge.w = 1.0;
        particle.color_radius = vec4(0.95, 0.18, 0.08, 0.35);
        particle.chemistry.x = 1.0;
        particle.chemistry.z = 1.0;
    } else if (species_roll < 8u) {
        particle.position_mass.w = 0.1;
        particle.velocity_charge.w = -1.0;
        particle.color_radius = vec4(0.12, 0.72, 1.0, 0.22);
        particle.chemistry.x = 3.0;
        particle.chemistry.z = 1.0;
    } else {
        particle.position_mass.w = 1.0;
        particle.color_radius = vec4(0.72, 0.72, 0.78, 0.35);
        particle.chemistry.x = 2.0;
        particle.chemistry.z = 1.0;
    }
    output_particles[destination] = particle;
}

void emit_commit(uint index) {
    if (index != 0u || params.event.x < 0.5) return;
    uint capacity = uint(params.event.z);
    uint available = capacity > output_count[0] ? capacity - output_count[0] : 0u;
    uint requested = uint(params.event.y);
    output_count[0] += min(requested, available);
}

void main() {
    uint index = gl_GlobalInvocationID.x;
    uint pass = uint(params.event.w);
    if (pass == 0u) clear_pass(index, 1u);
    else if (pass == 1u || pass == 4u) hash_pass(index);
    else if (pass == 2u) integrate_pass(index);
    else if (pass == 3u) clear_pass(index, 0u);
    else if (pass == 5u) merge_pass(index);
    else if (pass == 6u) compact_pass(index);
    else if (pass == 7u) emit_pass(index);
    else if (pass == 8u) emit_commit(index);
    else if (pass == 9u) render_clear_pass(index);
    else render_pass(index);
}
