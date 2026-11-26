use ash::vk;
use spirq::ReflectConfig;
use spirq::entry_point::ExecutionModel;
use spirq::ty::{DescriptorType, Type};
use spirq::var::Variable;

use super::layout::{
    Binding, FRAME_PUSH_BYTES, Limits, PushRange, SetPlan, StageBinding, StageInterval,
    merge_push_ranges, plan_sets,
};

const BINDLESS_TEXTURES: u32 = 0;
const BINDLESS_SAMPLERS: u32 = 1;
const BINDLESS_MATERIALS: u32 = 2;
const BINDLESS_PARAMS: u32 = 3;
const BINDLESS_WORLDS: u32 = 4;
const SAMPLER_COUNT: u32 = 4;

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
                bindings.push(StageBinding {
                    set: desc_bind.set(),
                    binding: desc_bind.bind(),
                    ty: descriptor_type(desc_ty)?,
                    count: *nbind,
                    runtime_array: *nbind == 0,
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

pub fn validate_bindless(interface: &Interface) -> Result<(), String> {
    if interface.sets.len() > 1 {
        return Err(format!(
            "shader declares descriptor set {}; only bindless set 0 is available",
            interface.sets.len() - 1
        ));
    }
    if let Some(set) = interface.sets.first() {
        for binding in set {
            match canonical(binding) {
                Ok(()) => {}
                Err(message) => return Err(message),
            }
        }
    }
    for range in &interface.push {
        let end = range
            .offset
            .checked_add(range.size)
            .ok_or_else(|| "push constant range overflows".to_string())?;
        if end > FRAME_PUSH_BYTES {
            return Err(format!(
                "push constant range {}..{end} is outside the frame block 0..{FRAME_PUSH_BYTES}",
                range.offset
            ));
        }
    }
    Ok(())
}

fn canonical(binding: &Binding) -> Result<(), String> {
    let (ty, runtime_array, count) = match binding.binding {
        BINDLESS_TEXTURES => (vk::DescriptorType::SAMPLED_IMAGE, true, 0),
        BINDLESS_SAMPLERS => (vk::DescriptorType::SAMPLER, false, SAMPLER_COUNT),
        BINDLESS_MATERIALS | BINDLESS_PARAMS | BINDLESS_WORLDS => {
            (vk::DescriptorType::STORAGE_BUFFER, false, 1)
        }
        other => {
            return Err(format!(
                "descriptor set 0 binding {other} is not part of the bindless layout"
            ));
        }
    };
    if binding.ty != ty {
        return Err(format!(
            "descriptor set 0 binding {} has type {:?}, bindless layout declares {ty:?}",
            binding.binding, binding.ty
        ));
    }
    if runtime_array {
        if !binding.runtime_array {
            return Err(format!(
                "descriptor set 0 binding {} must be a runtime array",
                binding.binding
            ));
        }
        return Ok(());
    }
    if binding.runtime_array {
        return Err(format!(
            "descriptor set 0 binding {} cannot be a runtime array",
            binding.binding
        ));
    }
    if binding.count != count {
        return Err(format!(
            "descriptor set 0 binding {} count is {}, bindless layout declares {count}",
            binding.binding, binding.count
        ));
    }
    Ok(())
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

    use super::{reflect_graphics, validate_bindless};
    use crate::shader::compile::{self, Optimize};
    use crate::shader::layout::{Binding, Limits};
    use crate::shader::words;

    #[test]
    fn reflects_the_bindless_frame_contract() {
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
        validate_bindless(&interface).expect("bindless contract");
        assert!(
            interface.vertex_inputs.is_empty(),
            "{:?}",
            interface.vertex_inputs
        );
        let set = interface.sets.first().expect("set 0");
        let textures = find_binding(set, 0);
        assert_eq!(textures.ty, vk::DescriptorType::SAMPLED_IMAGE);
        assert!(textures.runtime_array);
        assert_eq!(textures.count, 0);
        let samplers = find_binding(set, 1);
        assert_eq!(samplers.ty, vk::DescriptorType::SAMPLER);
        assert_eq!(samplers.count, 4);
        assert!(!samplers.runtime_array);
        assert_eq!(find_binding(set, 2).ty, vk::DescriptorType::STORAGE_BUFFER);
        assert_eq!(find_binding(set, 2).count, 1);
        assert_eq!(find_binding(set, 3).ty, vk::DescriptorType::STORAGE_BUFFER);
        assert_eq!(find_binding(set, 3).count, 1);
        assert_eq!(find_binding(set, 4).ty, vk::DescriptorType::STORAGE_BUFFER);
        assert_eq!(find_binding(set, 4).count, 1);
        assert!(
            interface
                .push
                .iter()
                .any(|range| range.offset == 0 && range.size >= 64),
            "{:?}",
            interface.push
        );
    }

    fn find_binding(set: &[Binding], binding: u32) -> &Binding {
        set.iter()
            .find(|slot| slot.binding == binding)
            .unwrap_or_else(|| panic!("missing binding {binding}"))
    }
}
