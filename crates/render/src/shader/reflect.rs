use ash::vk;
use spirq::ReflectConfig;
use spirq::entry_point::ExecutionModel;
use spirq::ty::{DescriptorType, Type};
use spirq::var::Variable;

use super::layout::{
    Binding, Limits, PushRange, SetPlan, StageBinding, StageInterval, merge_push_ranges, plan_sets,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Interface {
    pub push: Vec<PushRange>,
    pub sets: Vec<Vec<Binding>>,
    pub vertex_inputs: Vec<u32>,
}

pub fn reflect_graphics(
    vertex: &[u32],
    vertex_entry: &str,
    fragment: &[u32],
    fragment_entry: &str,
    limits: &Limits,
) -> Result<Interface, String> {
    let vertex_stage = reflect_stage(vertex, vertex_entry, vk::ShaderStageFlags::VERTEX)?;
    let fragment_stage = reflect_stage(fragment, fragment_entry, vk::ShaderStageFlags::FRAGMENT)?;
    if vertex_stage.execution != ExecutionModel::Vertex {
        return Err(format!(
            "{vertex_entry} is {:?}, expected a vertex shader",
            vertex_stage.execution
        ));
    }
    if fragment_stage.execution != ExecutionModel::Fragment {
        return Err(format!(
            "{fragment_entry} is {:?}, expected a fragment shader",
            fragment_stage.execution
        ));
    }

    let mut intervals = vertex_stage.intervals;
    intervals.extend(fragment_stage.intervals);
    let mut bindings = vertex_stage.bindings;
    bindings.extend(fragment_stage.bindings);
    let push = merge_push_ranges(&intervals, limits.max_push_constants_size)?;
    let SetPlan { sets } = plan_sets(&bindings, limits.max_bound_descriptor_sets)?;
    let mut vertex_inputs = vertex_stage.inputs;
    vertex_inputs.sort_unstable();
    vertex_inputs.dedup();
    Ok(Interface {
        push,
        sets,
        vertex_inputs,
    })
}

struct StageReflect {
    execution: ExecutionModel,
    intervals: Vec<StageInterval>,
    bindings: Vec<StageBinding>,
    inputs: Vec<u32>,
}

fn reflect_stage(
    words: &[u32],
    entry: &str,
    stage: vk::ShaderStageFlags,
) -> Result<StageReflect, String> {
    let mut config = ReflectConfig::new();
    let entries = config
        .spv(words)
        .reflect()
        .map_err(|err| format!("{entry}: {err}"))?;
    let found = entries
        .iter()
        .find(|candidate| candidate.name == entry)
        .ok_or_else(|| {
            let names: Vec<_> = entries
                .iter()
                .map(|candidate| candidate.name.as_str())
                .collect();
            format!("{entry} was not found in SPIR-V (entries: {names:?})")
        })?;

    let mut intervals = Vec::new();
    let mut bindings = Vec::new();
    let mut inputs = Vec::new();
    for variable in &found.vars {
        match variable {
            Variable::PushConstant { ty, .. } => {
                let (offset, size) = push_span(ty)?;
                intervals.push(StageInterval {
                    stage,
                    offset,
                    size,
                });
            }
            Variable::Descriptor {
                desc_bind,
                desc_ty,
                nbind,
                ..
            } => {
                if *nbind == 0 {
                    return Err(format!(
                        "{entry} binding {} uses an unbounded descriptor array",
                        desc_bind.bind()
                    ));
                }
                bindings.push(StageBinding {
                    set: desc_bind.set(),
                    binding: desc_bind.bind(),
                    ty: descriptor_type(desc_ty)?,
                    count: *nbind,
                    stages: stage,
                });
            }
            Variable::Input { location, .. } if stage == vk::ShaderStageFlags::VERTEX => {
                inputs.push(location.loc());
            }
            Variable::Input { .. } | Variable::Output { .. } | Variable::SpecConstant { .. } => {}
        }
    }

    Ok(StageReflect {
        execution: found.exec_model,
        intervals,
        bindings,
        inputs,
    })
}

fn push_span(ty: &Type) -> Result<(u32, u32), String> {
    let structure = ty
        .as_struct()
        .ok_or_else(|| "push constant is not a struct".to_string())?;
    if structure.members.is_empty() {
        return Err("push constant struct is empty".into());
    }
    let mut min_offset = None;
    let mut max_end = 0u32;
    for member in &structure.members {
        let offset = u32::try_from(
            member
                .offset
                .ok_or_else(|| "push constant member has no offset".to_string())?,
        )
        .map_err(|_| "push constant offset does not fit in u32".to_string())?;
        let size = u32::try_from(
            member
                .ty
                .nbyte()
                .ok_or_else(|| "push constant member has no size".to_string())?,
        )
        .map_err(|_| "push constant member size does not fit in u32".to_string())?;
        min_offset = Some(min_offset.map_or(offset, |current: u32| current.min(offset)));
        max_end = max_end.max(offset.saturating_add(size));
    }
    let offset = min_offset.unwrap_or(0);
    let size = max_end.saturating_sub(offset).div_ceil(4) * 4;
    Ok((offset, size))
}

fn descriptor_type(ty: &DescriptorType) -> Result<vk::DescriptorType, String> {
    Ok(match ty {
        DescriptorType::Sampler() => vk::DescriptorType::SAMPLER,
        DescriptorType::CombinedImageSampler() => vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
        DescriptorType::SampledImage() => vk::DescriptorType::SAMPLED_IMAGE,
        DescriptorType::StorageImage(_) => vk::DescriptorType::STORAGE_IMAGE,
        DescriptorType::UniformTexelBuffer() => vk::DescriptorType::UNIFORM_TEXEL_BUFFER,
        DescriptorType::StorageTexelBuffer(_) => vk::DescriptorType::STORAGE_TEXEL_BUFFER,
        DescriptorType::UniformBuffer() => vk::DescriptorType::UNIFORM_BUFFER,
        DescriptorType::StorageBuffer(_) => vk::DescriptorType::STORAGE_BUFFER,
        DescriptorType::InputAttachment(_) => vk::DescriptorType::INPUT_ATTACHMENT,
        DescriptorType::AccelStruct() => {
            return Err("acceleration structure descriptors are not supported".into());
        }
    })
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use ash::vk;

    use super::reflect_graphics;
    use crate::shader::compile::{self, Optimize};
    use crate::shader::layout::{Limits, PushRange};
    use crate::shader::words;

    #[test]
    fn reflects_triangle_as_a_vertex_push_constant() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("shaders");
        let temp = std::env::temp_dir().join("engine-shader-reflect");
        fs::create_dir_all(&temp).expect("temp dir");
        let vertex_path = temp.join("triangle.vert.spv");
        let fragment_path = temp.join("triangle.frag.spv");
        compile::compile_stage(
            &dir.join("triangle.slang"),
            "vertexMain",
            "vertex",
            &dir,
            Optimize::Debug,
            &vertex_path,
        )
        .expect("vertex slangc");
        compile::compile_stage(
            &dir.join("triangle.slang"),
            "fragmentMain",
            "fragment",
            &dir,
            Optimize::Debug,
            &fragment_path,
        )
        .expect("fragment slangc");
        let vertex = words::spirv_words(&fs::read(&vertex_path).expect("read vertex"))
            .expect("vertex words");
        let fragment = words::spirv_words(&fs::read(&fragment_path).expect("read fragment"))
            .expect("fragment words");
        let interface = reflect_graphics(
            &vertex,
            "vertexMain",
            &fragment,
            "fragmentMain",
            &Limits {
                max_push_constants_size: 128,
                max_bound_descriptor_sets: 4,
            },
        )
        .expect("reflect");
        assert!(interface.sets.is_empty(), "{:?}", interface.sets);
        assert!(
            interface.vertex_inputs.is_empty(),
            "{:?}",
            interface.vertex_inputs
        );
        assert_eq!(
            interface.push,
            vec![PushRange {
                stages: vk::ShaderStageFlags::VERTEX,
                offset: 0,
                size: 64,
            }]
        );
    }
}
