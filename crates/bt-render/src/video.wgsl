// The playing-video layer: one textured quad, masked by a rounded box.
//
// Two things this draws and the `math` shader beside it cannot. The first is a
// flat colour from the same pipeline as the picture — the letterbox ground,
// which has to be rounded by the *same* mask or a float would show four square
// corners under a rounded picture. The second is the mask itself: a sampled
// texture cannot be covered by the coverage-weighted rectangle runs
// `rounded_overlay_fill` paints a rounded colour with, so the rounding is a
// signed distance computed per fragment here.
//
// `@builtin(position)` in the fragment stage is the framebuffer pixel, whatever
// viewport the pass is set to — which is why the box travels in whole-surface
// pixels and needs no transform of its own.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    // Flat, all of them: they describe the quad, not a gradient across it.
    @location(1) @interpolate(flat) tint: vec4<f32>,
    @location(2) @interpolate(flat) texture_weight: f32,
    @location(3) @interpolate(flat) box_center: vec2<f32>,
    @location(4) @interpolate(flat) box_half: vec2<f32>,
    @location(5) @interpolate(flat) radius: f32,
};

@group(0) @binding(0)
var video_texture: texture_2d<f32>;

@group(0) @binding(1)
var video_sampler: sampler;

@vertex
fn vertex(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) tint: vec4<f32>,
    @location(3) texture_weight: f32,
    @location(4) box_center: vec2<f32>,
    @location(5) box_half: vec2<f32>,
    @location(6) radius: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = uv;
    output.tint = tint;
    output.texture_weight = texture_weight;
    output.box_center = box_center;
    output.box_half = box_half;
    output.radius = radius;
    return output;
}

// The signed distance from `point` to a box of half-extent `half` whose corners
// are rounded by `radius`: negative inside, zero on the edge, positive outside.
fn rounded_box_distance(point: vec2<f32>, half: vec2<f32>, radius: f32) -> f32 {
    let inner = half - vec2<f32>(radius, radius);
    let outside = abs(point) - inner;
    return length(max(outside, vec2<f32>(0.0, 0.0))) + min(max(outside.x, outside.y), 0.0) - radius;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(video_texture, video_sampler, input.uv);
    // `texture_weight` is 1 for the picture and 0 for the ground. The texture is
    // sampled either way — a bind group is always bound and a branch here would
    // buy nothing — and the mix is what decides which of the two is drawn.
    let colour = mix(input.tint.rgb, sampled.rgb, input.texture_weight);
    // Coverage across one pixel, which is what turns the distance into an edge
    // that is not a staircase. A radius of zero makes this exactly the quad's
    // own rectangle, so a pane pays nothing for a float's corner.
    let distance = rounded_box_distance(
        input.position.xy - input.box_center,
        input.box_half,
        input.radius,
    );
    let coverage = clamp(0.5 - distance, 0.0, 1.0);
    // A frame of video carries no transparency; `tint.a` is the layer's own
    // opacity and the coverage is the mask, so the two multiply and the colour
    // is left alone — the same arithmetic, and the same reason for it, as the
    // `math` shader's note about fading toward the backdrop rather than toward
    // black.
    return vec4<f32>(colour, input.tint.a * coverage);
}
