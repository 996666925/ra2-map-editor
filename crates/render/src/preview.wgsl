struct View {
    scale: vec2<f32>,
    translation: vec2<f32>,
    srgb_target: u32,
    pad0: u32,
    pad1: u32,
    pad2: u32,
};
@group(0) @binding(0) var image: texture_2d<f32>;
@group(0) @binding(1) var nearest_sampler: sampler;
@group(0) @binding(2) var<uniform> view: View;

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex fn vs_main(@builtin(vertex_index) index: u32) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0)
    );
    let uv = corners[index];
    let clip = vec2(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0);
    var out: VertexOutput;
    out.position = vec4(clip * view.scale + view.translation, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var color = textureSample(image, nearest_sampler, in.uv);
    // egui normally uses a gamma-space UNORM target. Account for an sRGB
    // surface if the host selects one, rather than applying gamma twice.
    if view.srgb_target != 0u {
        let low = color.rgb / 12.92;
        let high = pow((color.rgb + vec3(0.055)) / 1.055, vec3(2.4));
        color = vec4(select(high, low, color.rgb <= vec3(0.04045)), color.a);
    }
    return color;
}
