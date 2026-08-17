// The window's ground, as one quad under everything — docs/DESIGN.md §7.1.6c-4b.
//
// This shader does not composite onto the clear, it REPLACES it: the pipeline's
// blend is `Replace`, and the fragment writes the finished ground, premultiplied
// by the ground's own alpha. That is what keeps one arithmetic in one place —
// the clear and this quad produce the same value for the same ground, and a
// window with no picture simply skips the quad.
//
// Everything here is linear light. The texture is `Rgba8UnormSrgb`, so
// `textureSample` has already decoded it; the target is sRGB, so wgpu encodes
// the result once on the way out. Mixing in between is therefore mixing light,
// which is the only place a 50% blend of two colours means what it says.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Linear RGB of the scheme's background, and the ground's alpha in `.a`.
    @location(1) ground: vec4<f32>,
    @location(2) image_opacity: f32,
};

@group(0) @binding(0)
var background_texture: texture_2d<f32>;

@group(0) @binding(1)
var background_sampler: sampler;

@vertex
fn vertex(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) ground: vec4<f32>,
    @location(3) image_opacity: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = uv;
    output.ground = ground;
    output.image_opacity = image_opacity;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(background_texture, background_sampler, input.uv);
    // How much picture there is at this texel: the row's percentage, scaled by
    // the picture's own alpha. A PNG with a transparent corner shows the
    // scheme's background through that corner at every setting, which is the
    // only reading of "transparent" that is not a second opacity control.
    let presence = sampled.a * input.image_opacity;
    let ground = mix(input.ground.rgb, sampled.rgb, presence);
    // Premultiplied, because the swapchain is `PreMultiplied` (§2.3 A2).
    return vec4<f32>(ground * input.ground.a, input.ground.a);
}
