#include "bindless.slang"

struct Varying {
    float4 position : SV_Position;
    [[vk::location(0)]] float3 local_pos;
};

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

[shader("vertex")]
Varying vertexMain(uint vertexId : SV_VertexID) {
    Varying output;
    float3 position = corners[indices[vertexId]];
    output.position = mul(frame.view_proj, float4(position, 1.0));
    output.local_pos = position;
    return output;
}

float2 face_uv(float3 position) {
    float3 absolute = abs(position);
    float2 uv = position.xy;
    if (absolute.x >= absolute.y && absolute.x >= absolute.z) {
        uv = position.zy;
    } else if (absolute.y >= absolute.z) {
        uv = position.xz;
    }
    return uv + 0.5;
}

[shader("fragment")]
float4 fragmentMain(Varying input) : SV_Target {
    MaterialRef material = materials[frame.material_index];
    if (material.type_id == 0) {
        return float4(1.0, 0.0, 1.0, 1.0);
    }
    float4 albedo = sample_texture(
        material.textures[0],
        material.samplers[0],
        face_uv(input.local_pos));
    float4 color = material_params.Load<UnlitParams>(material.param_offset).color;
    return float4(albedo.rgb * color.rgb, 1.0);
}
