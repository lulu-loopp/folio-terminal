// A fading overlay surface, composited once (`docs/DESIGN.md`, 2026-09-24:
// overlay group opacity, composited like CSS).
//
// The surface was drawn at full strength into a texture of its own, with the
// blending every overlay pipeline uses. This puts that texture back on the
// frame through a **non-sRGB view**, so the blend below runs on encoded bytes —
// which is what a browser does with `opacity` on an element.
//
// Two ways back, because a surface's pixels are of two kinds:
//
// * `over` — everything a surface lays *on* the window. The texel is the
//   surface's own premultiplied colour in linear light; it is un-premultiplied,
//   encoded, and premultiplied again in encoded space, so a translucent edge
//   meets the frame the way the CSS mock-up's does.
// * `cross_fade` — a **ground**: pixels that *are* the window (`OverlayGround`).
//   The texel's bytes are exactly what a ground drawn straight onto the frame
//   would have written there, so they are taken raw and lerped against the frame
//   by the pass's blend constant, alpha channel included — the one-translucency
//   ruling, now for the whole surface standing on the ground.
//
// `@builtin(position)` is the frame's pixel whatever the viewport; the surface
// was drawn at the frame's own coordinates, so the texel is the pixel minus the
// group's whole-pixel offset.

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) offset: vec2<f32>,
    @location(1) @interpolate(flat) opacity: f32,
};

@group(0) @binding(0)
var surface_texture: texture_2d<f32>;

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) offset: vec2<f32>,
    @location(2) opacity: f32,
) -> VertexOutput {
    let corners = array<vec2<f32>, 6>(
        vec2<f32>(rect.x, rect.y),
        vec2<f32>(rect.x, rect.w),
        vec2<f32>(rect.z, rect.w),
        vec2<f32>(rect.x, rect.y),
        vec2<f32>(rect.z, rect.w),
        vec2<f32>(rect.z, rect.y),
    );
    var output: VertexOutput;
    output.position = vec4<f32>(corners[vertex_index], 0.0, 1.0);
    output.offset = offset;
    output.opacity = opacity;
    return output;
}

fn texel(input: VertexOutput) -> vec4<f32> {
    let at = vec2<i32>(floor(input.position.xy - input.offset));
    return textureLoad(surface_texture, at, 0);
}

// **Straight-line code, no branch on a pixel's value** (T-WEBVIEW-WARMUP-DROPS-
// THE-DEVICE). Written with `if … return`, the transfer below and the empty
// texel's early way out compiled — under FXC's optimiser, which a release build
// uses — to a shader that removed the device on Windows 10's Basic Render
// Driver the first time a surface was put back. Both halves of the transfer are
// computed and `select` keeps one; it is the same function on every input.

// The sRGB transfer, IEC 61966-2-1 — the inverse of the decode the sRGB view
// performed on the load.
fn encode(linear: vec3<f32>) -> vec3<f32> {
    let low = linear * 12.92;
    let high = 1.055 * pow(linear, vec3<f32>(1.0 / 2.4)) - 0.055;
    return select(high, low, linear <= vec3<f32>(0.0031308));
}

@fragment
fn over(input: VertexOutput) -> @location(0) vec4<f32> {
    let premultiplied = texel(input);
    let alpha = premultiplied.a;
    // Un-premultiplied over an alpha kept off zero: every 8-bit alpha but zero
    // is at least 1/255 and divides as itself, and an empty texel — colour zero
    // over alpha zero — comes out empty without a NaN to throw away.
    // Clamped: an 8-bit texel's colour over its own 8-bit alpha can round a hair past one.
    let linear = clamp(premultiplied.rgb / max(alpha, 1.0 / 255.0), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(encode(linear) * alpha, alpha) * input.opacity;
}

@fragment
fn cross_fade(input: VertexOutput) -> @location(0) vec4<f32> {
    return texel(input) * input.opacity;
}
