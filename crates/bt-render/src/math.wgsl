struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) opacity: f32,
};

@group(0) @binding(0)
var math_texture: texture_2d<f32>;

@group(0) @binding(1)
var math_sampler: sampler;

@vertex
fn vertex(
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) opacity: f32,
) -> VertexOutput {
    var output: VertexOutput;
    output.position = vec4<f32>(position, 0.0, 1.0);
    output.uv = uv;
    output.opacity = opacity;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    let sampled = textureSample(math_texture, math_sampler, input.uv);
    // The raster carries straight (unpremultiplied) alpha and the pipeline
    // blends with `ALPHA_BLENDING`, so an element-wide opacity is exactly a
    // scale on that alpha: the colour is untouched and only its coverage of the
    // backdrop changes. Scaling the colour instead would darken the mark toward
    // black as it faded, rather than fading it toward whatever is behind it.
    return vec4<f32>(sampled.rgb, sampled.a * input.opacity);
}
