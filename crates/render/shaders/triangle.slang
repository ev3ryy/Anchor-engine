struct Varying {
    float4 position : SV_Position;
    [[vk::location(0)]] float3 color;
};

struct Camera {
    column_major float4x4 view_proj;
};

[[vk::push_constant]]
Camera camera;

static const float3 corners[8] = {
    float3(-0.5, -0.5, -0.5),
    float3(0.5, -0.5, -0.5),
    float3(0.5, 0.5, -0.5),
    float3(-0.5, 0.5, -0.5),
    float3(-0.5, -0.5, 0.5),
    float3(0.5, -0.5, 0.5),
    float3(0.5, 0.5, 0.5),
    float3(-0.5, 0.5, 0.5),
};

// Outward faces, counter-clockwise when viewed from outside.
static const uint indices[36] = {
    4, 6, 7, 4, 5, 6, // +Z
    1, 0, 3, 1, 3, 2, // -Z
    5, 1, 2, 5, 2, 6, // +X
    0, 4, 7, 0, 7, 3, // -X
    3, 7, 6, 3, 6, 2, // +Y
    0, 1, 5, 0, 5, 4, // -Y
};

static const float3 face_colors[6] = {
    float3(1.0, 0.0, 0.0),
    float3(0.0, 1.0, 0.0),
    float3(0.0, 0.0, 1.0),
    float3(1.0, 1.0, 0.0),
    float3(0.0, 1.0, 1.0),
    float3(1.0, 0.0, 1.0),
};

[shader("vertex")]
Varying vertexMain(uint vertexId : SV_VertexID) {
    Varying output;
    float3 position = corners[indices[vertexId]];
    output.position = mul(camera.view_proj, float4(position, 1.0));
    output.color = face_colors[vertexId / 6];
    return output;
}

[shader("fragment")]
float4 fragmentMain(Varying input) : SV_Target {
    return float4(input.color, 1.0);
}
