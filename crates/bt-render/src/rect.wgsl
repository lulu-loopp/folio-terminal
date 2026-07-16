struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

@vertex
fn vertex(
    @builtin(vertex_index) vertex_index: u32,
    @location(0) rect: vec4<f32>,
    @location(1) color: vec4<f32>,
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
    output.color = color;
    return output;
}

@fragment
fn fragment(input: VertexOutput) -> @location(0) vec4<f32> {
    return input.color;
}
