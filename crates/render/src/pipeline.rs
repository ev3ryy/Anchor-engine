use std::io::Cursor;

use ash::{Device, vk};

use crate::Error;

pub struct PipelineBundle {
    device: Device,
    layout: vk::PipelineLayout,
    pub pipeline: vk::Pipeline,
    format: vk::Format,
}

impl PipelineBundle {
    pub fn format(&self) -> vk::Format {
        self.format
    }
}

impl Drop for PipelineBundle {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_pipeline(self.pipeline, None);
            self.device.destroy_pipeline_layout(self.layout, None);
        }
    }
}

pub fn create(device: &Device, format: vk::Format) -> Result<PipelineBundle, Error> {
    let vert_code = read_spirv(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/triangle.vert.spv"
    )))?;
    let frag_code = read_spirv(include_bytes!(concat!(
        env!("OUT_DIR"),
        "/triangle.frag.spv"
    )))?;
    let mut vert_module = vk::ShaderModuleCreateInfo::default().code(&vert_code);
    let mut frag_module = vk::ShaderModuleCreateInfo::default().code(&frag_code);
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .name(c"vertexMain")
            .push_next(&mut vert_module),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .name(c"fragmentMain")
            .push_next(&mut frag_module),
    ];

    let layout =
        unsafe { device.create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)? };

    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST)
        .primitive_restart_enable(false);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let color_attachment = vk::PipelineColorBlendAttachmentState::default().color_write_mask(
        vk::ColorComponentFlags::R
            | vk::ColorComponentFlags::G
            | vk::ColorComponentFlags::B
            | vk::ColorComponentFlags::A,
    );
    let color_blend = vk::PipelineColorBlendStateCreateInfo::default()
        .attachments(std::slice::from_ref(&color_attachment));
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let mut rendering = vk::PipelineRenderingCreateInfo::default()
        .color_attachment_formats(std::slice::from_ref(&format));
    let pipeline_info = vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
        .push_next(&mut rendering);

    let pipeline = match unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &[pipeline_info], None)
    } {
        Ok(mut pipelines) => pipelines.remove(0),
        Err((pipelines, err)) => {
            unsafe {
                for pipeline in pipelines {
                    if pipeline != vk::Pipeline::null() {
                        device.destroy_pipeline(pipeline, None);
                    }
                }
                device.destroy_pipeline_layout(layout, None);
            }
            return Err(err.into());
        }
    };

    Ok(PipelineBundle {
        device: device.clone(),
        layout,
        pipeline,
        format,
    })
}

fn read_spirv(bytes: &[u8]) -> Result<Vec<u32>, Error> {
    ash::util::read_spv(&mut Cursor::new(bytes)).map_err(Error::Shader)
}
