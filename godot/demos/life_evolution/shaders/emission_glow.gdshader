shader_type spatial;
render_mode blend_add, depth_draw_opaque;

// Simple emission glow shader - for particles with energy states.
// Creates a soft glow with falloff.

uniform vec3 base_color : source_color = vec3(0.4, 0.7, 1.0);
uniform float intensity : hint_range(0.0, 10.0) = 2.0;
uniform float fresnel_power : hint_range(0.1, 10.0) = 2.0;
uniform float pulse_speed : hint_range(0.0, 5.0) = 0.0;
uniform float pulse_strength : hint_range(0.0, 1.0) = 0.0;

void fragment() {
    // Fresnel effect for edge glow
    float fresnel = pow(1.0 - dot(NORMAL, VIEW_DIRECTION), fresnel_power);
    
    // Optional pulsing
    float pulse = 1.0;
    if (pulse_speed > 0.0) {
        pulse = 1.0 + sin(TIME * pulse_speed) * pulse_strength;
    }
    
    // Output
    ALBEDO = vec3(0.0);
    EMISSION = base_color * intensity * pulse * (0.5 + fresnel * 0.5);
    ALPHA = 1.0;
}
