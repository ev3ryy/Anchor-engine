#include "bindless.slang"

struct WorldMatrix {
    column_major float4x4 value;
};

[[vk::binding(4, 0)]]
StructuredBuffer<WorldMatrix> worlds;

struct Varying {
    float4 position : SV_Position;
    [[vk::location(0)]] float3 normal;
    [[vk::location(1)]] float2 uv;
};

[shader("vertex")]
Varying vertexMain(
    [[vk::location(0)]] float3 position : POSITION,
    [[vk::location(1)]] float3 normal : NORMAL,
    [[vk::location(2)]] float2 uv : TEXCOORD,
    uint instanceId : SV_InstanceID
) {
    Varying output;
    float4x4 world = worlds[frame.instance_base + instanceId].value;
    // `precise` keeps the normal attribute live for the shared mesh format.
    precise float3 kept = normal * 0.0;
    output.position = mul(frame.view_proj, mul(world, float4(position + kept, 1.0)));
    output.normal = normal;
    output.uv = uv;
    return output;
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
        input.uv);
    float4 color = material_params.Load<UnlitParams>(material.param_offset).color;
    precise float3 normal = input.normal;
    precise float keep = dot(normal, normal) * 0.0 + 1.0;
    return float4(albedo.rgb * color.rgb * keep, 1.0);
}
