use ash::vk;

use super::layout::PushContract;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BlendMode {
    Opaque,
    Alpha,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GraphicsState {
    pub topology: vk::PrimitiveTopology,
    pub cull: vk::CullModeFlags,
    pub front_face: vk::FrontFace,
    pub depth_test: bool,
    pub depth_write: bool,
    pub depth_compare: vk::CompareOp,
    pub blend: BlendMode,
}

impl GraphicsState {
    pub fn opaque_mesh() -> Self {
        Self {
            topology: vk::PrimitiveTopology::TRIANGLE_LIST,
            cull: vk::CullModeFlags::BACK,
            front_face: vk::FrontFace::COUNTER_CLOCKWISE,
            depth_test: true,
            depth_write: true,
            depth_compare: vk::CompareOp::LESS,
            blend: BlendMode::Opaque,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ProgramDesc {
    pub state: GraphicsState,
    pub push: Option<PushContract>,
}

pub fn description(name: &str) -> ProgramDesc {
    match name {
        "triangle" => ProgramDesc {
            state: GraphicsState::opaque_mesh(),
            push: Some(PushContract {
                offset: 0,
                size: std::mem::size_of::<[f32; 16]>() as u32,
            }),
        },
        _ => ProgramDesc {
            state: GraphicsState::opaque_mesh(),
            push: None,
        },
    }
}
