[[vk::binding(0, 0)]]
Texture2D textures[];

[[vk::binding(1, 0)]]
SamplerState samplers[4];

struct MaterialRef {
    uint type_id;
    uint flags;
    uint param_offset;
    uint param_size;
    uint textures[8];
    uint samplers[8];
};

[[vk::binding(2, 0)]]
StructuredBuffer<MaterialRef> materials;

[[vk::binding(3, 0)]]
ByteAddressBuffer material_params;

struct FramePush {
    column_major float4x4 view_proj;
    uint material_index;
    uint instance_base;
    uint pad1;
    uint pad2;
};

[[vk::push_constant]]
FramePush frame;

struct UnlitParams {
    float4 color;
};

static const uint MATERIAL_UNLIT = 1;

float4 sample_texture(uint texture_index, uint sampler_index, float2 uv) {
    uint texture = NonUniformResourceIndex(texture_index);
    uint sampler = NonUniformResourceIndex(sampler_index);
    return textures[texture].Sample(samplers[sampler], uv);
}
