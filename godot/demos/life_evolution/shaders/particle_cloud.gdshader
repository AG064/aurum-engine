shader_type spatial;
render_mode blend_add, depth_draw_opaque, cull_disabled, shadows_disabled, ambient_light_disabled;

// Particle cloud shader - used for visualizing large numbers of particles.
// Each particle is rendered as a billboard with custom color and emission.

uniform vec3 emission_color : source_color = vec3(1.0, 0.7, 0.3);
uniform float emission_strength : hint_range(0.0, 10.0) = 1.5;
uniform float point_size : hint_range(0.1, 100.0) = 5.0;
uniform float fade_distance : hint_range(1.0, 10000.0) = 5000.0;
uniform float glow_falloff : hint_range(0.1, 10.0) = 2.0;
uniform vec3 camera_position;

void vertex() {
    // Billboard the particle to face the camera
    MODELVIEW_MATRIX = VIEW_MATRIX * mat4(
        INV_VIEW_MATRIX[0],
        INV_VIEW_MATRIX[1],
        INV_VIEW_MATRIX[2],
        MODEL_MATRIX[3]
    );
    
    // Scale based on distance
    float scale_factor = point_size;
    MODELVIEW_MATRIX[0] *= scale_factor;
    MODELVIEW_MATRIX[1] *= scale_factor;
    MODELVIEW_MATRIX[2] *= scale_factor;
}

void fragment() {
    // Create a circular falloff
    vec2 uv_centered = UV - vec2(0.5);
    float dist = length(uv_centered);
    float alpha = 1.0 - smoothstep(0.0, 0.5, dist);
    
    // Color with emission
    ALBEDO = vec3(0.0);
    EMISSION = emission_color * emission_strength * alpha;
    ALPHA = alpha;
}
