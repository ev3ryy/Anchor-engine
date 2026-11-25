use std::ffi::CStr;

use ash::{Device, vk};

use crate::Error;
use crate::shader::state::{BlendMode, GraphicsState};

pub struct Modules {
    pub vertex: vk::ShaderModule,
    pub fragment: vk::ShaderModule,
}

pub fn create_modules(device: &Device, vertex: &[u32], fragment: &[u32]) -> Result<Modules, Error> {
    let vertex_info = vk::ShaderModuleCreateInfo::default().code(vertex);
    let vertex = unsafe { device.create_shader_module(&vertex_info, None)? };
    let fragment_info = vk::ShaderModuleCreateInfo::default().code(fragment);
    let fragment = match unsafe { device.create_shader_module(&fragment_info, None) } {
        Ok(fragment) => fragment,
        Err(err) => {
            unsafe { device.destroy_shader_module(vertex, None) };
            return Err(err.into());
        }
    };
    Ok(Modules { vertex, fragment })
}

pub fn destroy_module(device: &Device, module: vk::ShaderModule) {
    unsafe { device.destroy_shader_module(module, None) };
}

pub fn create_graphics(
    device: &Device,
    cache: vk::PipelineCache,
    modules: &Modules,
    vertex_entry: &CStr,
    fragment_entry: &CStr,
    layout: vk::PipelineLayout,
    state: &GraphicsState,
    color_format: vk::Format,
    depth_format: vk::Format,
    spec: &[(u32, u32)],
) -> Result<vk::Pipeline, Error> {
    let spec_entries: Vec<vk::SpecializationMapEntry> = spec
        .iter()
        .enumerate()
        .map(|(index, (id, _))| {
            vk::SpecializationMapEntry::default()
                .constant_id(*id)
                .offset((index * 4) as u32)
                .size(4)
        })
        .collect();
    let spec_data: Vec<u8> = spec
        .iter()
        .flat_map(|(_, value)| value.to_le_bytes())
        .collect();
    let spec_info = vk::SpecializationInfo::default()
        .map_entries(&spec_entries)
        .data(&spec_data);
    let spec_info = (!spec.is_empty()).then_some(spec_info);

    let mut vertex_stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::VERTEX)
        .module(modules.vertex)
        .name(vertex_entry);
    let mut fragment_stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::FRAGMENT)
        .module(modules.fragment)
        .name(fragment_entry);
    if let Some(spec_info) = spec_info.as_ref() {
        vertex_stage = vertex_stage.specialization_info(spec_info);
        fragment_stage = fragment_stage.specialization_info(spec_info);
    }
    let stages = [vertex_stage, fragment_stage];

    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(state.topology)
        .primitive_restart_enable(false);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(state.cull)
        .front_face(state.front_face)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let mut color_attachment = vk::PipelineColorBlendAttachmentState::default().color_write_mask(
        vk::ColorComponentFlags::R
            | vk::ColorComponentFlags::G
            | vk::ColorComponentFlags::B
            | vk::ColorComponentFlags::A,
    );
    if state.blend == BlendMode::Alpha {
        color_attachment = color_attachment
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD);
    }
    let depth_stencil = vk::PipelineDepthStencilStateCreateInfo::default()
        .depth_test_enable(state.depth_test)
        .depth_write_enable(state.depth_write)
        .depth_compare_op(state.depth_compare);
    let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(std::slice::from_ref(&color_attachment));
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let mut rendering = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(std::slice::from_ref(&color_format))
        .depth_attachment_format(depth_format);
    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .depth_stencil_state(&depth_stencil)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
        .push_next(&mut rendering);

    match unsafe { device.create_graphics_pipelines(cache, &[pipeline_info], None) } {
        Ok(mut pipelines) => Ok(pipelines.remove(0)),
        Err((pipelines, err)) => {
            unsafe {
                for pipeline in pipelines {
                    if pipeline != vk::Pipeline::null() {
                        device.destroy_pipeline(pipeline, None);
                    }
                }
            }
            Err(err.into())
        }
    }
}
