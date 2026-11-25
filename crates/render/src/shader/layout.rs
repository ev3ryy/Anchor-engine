use ash::vk;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Limits {
    pub max_push_constants_size: u32,
    pub max_bound_descriptor_sets: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StageInterval {
    pub stage: vk::ShaderStageFlags,
    pub offset: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PushRange {
    pub stages: vk::ShaderStageFlags,
    pub offset: u32,
    pub size: u32,
}

impl PushRange {
    fn end(self) -> Result<u32, String> {
        self.offset
            .checked_add(self.size)
            .ok_or_else(|| "push constant range overflows".to_string())
    }
}

#[cfg_attr(not(test), allow(dead_code))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PushContract {
    pub offset: u32,
    pub size: u32,
}

pub const FRAME_PUSH_BYTES: u32 = 80;

pub fn frame_push_stages() -> vk::ShaderStageFlags {
    vk::ShaderStageFlags::VERTEX | vk::ShaderStageFlags::FRAGMENT
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct StageBinding {
    pub set: u32,
    pub binding: u32,
    pub ty: vk::DescriptorType,
    pub count: u32,
    pub runtime_array: bool,
    pub stages: vk::ShaderStageFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Binding {
    pub binding: u32,
    pub ty: vk::DescriptorType,
    pub count: u32,
    pub runtime_array: bool,
    pub stages: vk::ShaderStageFlags,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetPlan {
    pub sets: Vec<Vec<Binding>>,
}

pub fn merge_push_ranges(
    intervals: &[StageInterval],
    limit: u32,
) -> Result<Vec<PushRange>, String> {
    let mut per_stage: Vec<PushRange> = Vec::new();
    for interval in intervals {
        if interval.size == 0 || interval.offset % 4 != 0 || interval.size % 4 != 0 {
            return Err("push constant offset and size must be non-zero multiples of 4".into());
        }
        let end = interval
            .offset
            .checked_add(interval.size)
            .ok_or_else(|| "push constant range overflows".to_string())?;
        if let Some(existing) = per_stage
            .iter_mut()
            .find(|range| range.stages == interval.stage)
        {
            let existing_end = existing.end()?;
            let start = existing.offset.min(interval.offset);
            let end = existing_end.max(end);
            existing.offset = start;
            existing.size = end - start;
        } else {
            per_stage.push(PushRange {
                stages: interval.stage,
                offset: interval.offset,
                size: interval.size,
            });
        }
    }

    let mut ranges = per_stage;
    loop {
        let mut merged = false;
        let mut left = 0;
        while left < ranges.len() {
            let mut right = left + 1;
            while right < ranges.len() {
                let overlap = ranges[left].offset < ranges[right].end()?
                    && ranges[right].offset < ranges[left].end()?;
                let same = ranges[left].offset == ranges[right].offset
                    && ranges[left].size == ranges[right].size;
                if overlap && !same {
                    let start = ranges[left].offset.min(ranges[right].offset);
                    let end = ranges[left].end()?.max(ranges[right].end()?);
                    let stages = ranges[left].stages | ranges[right].stages;
                    ranges.remove(right);
                    ranges.remove(left);
                    ranges.push(PushRange {
                        stages,
                        offset: start,
                        size: end - start,
                    });
                    merged = true;
                    break;
                }
                right += 1;
            }
            if merged {
                break;
            }
            left += 1;
        }
        if !merged {
            break;
        }
    }

    ranges.sort_by_key(|range| (range.offset, range.size));
    let mut collapsed: Vec<PushRange> = Vec::new();
    for range in ranges {
        if let Some(last) = collapsed.last_mut() {
            if last.offset == range.offset && last.size == range.size {
                last.stages |= range.stages;
                continue;
            }
        }
        collapsed.push(range);
    }

    for range in &collapsed {
        let end = range.end()?;
        if range.size == 0 || range.offset % 4 != 0 || range.size % 4 != 0 || end > limit {
            return Err(format!(
                "push constant range {}..{end} exceeds device limit {limit} or is misaligned",
                range.offset
            ));
        }
    }
    Ok(collapsed)
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn covering(ranges: &[PushRange], contract: PushContract) -> Option<&PushRange> {
    let end = contract.offset.checked_add(contract.size)?;
    ranges.iter().find(|range| {
        range.offset <= contract.offset && range.end().is_ok_and(|range_end| range_end >= end)
    })
}

pub fn plan_sets(bindings: &[StageBinding], max_sets: u32) -> Result<SetPlan, String> {
    if bindings.is_empty() {
        return Ok(SetPlan { sets: Vec::new() });
    }
    let max_set = bindings
        .iter()
        .map(|binding| binding.set)
        .max()
        .unwrap_or(0);
    let count = max_set
        .checked_add(1)
        .ok_or_else(|| "descriptor set index overflows".to_string())?;
    if count > max_sets {
        return Err(format!(
            "descriptor set {max_set} exceeds maxBoundDescriptorSets {max_sets}"
        ));
    }
    let mut sets: Vec<Vec<Binding>> = vec![Vec::new(); count as usize];
    for binding in bindings {
        let set = &mut sets[binding.set as usize];
        if let Some(existing) = set.iter_mut().find(|slot| slot.binding == binding.binding) {
            if existing.ty != binding.ty
                || existing.count != binding.count
                || existing.runtime_array != binding.runtime_array
            {
                return Err(format!(
                    "descriptor set {} binding {} is declared with different types",
                    binding.set, binding.binding
                ));
            }
            existing.stages |= binding.stages;
        } else {
            set.push(Binding {
                binding: binding.binding,
                ty: binding.ty,
                count: binding.count,
                runtime_array: binding.runtime_array,
                stages: binding.stages,
            });
        }
    }
    for set in &mut sets {
        set.sort_by_key(|binding| binding.binding);
    }
    Ok(SetPlan { sets })
}

#[cfg(test)]
mod tests {
    use ash::vk;

    use super::{
        PushContract, PushRange, StageBinding, StageInterval, covering, merge_push_ranges,
        plan_sets,
    };

    const VERTEX: vk::ShaderStageFlags = vk::ShaderStageFlags::VERTEX;
    const FRAGMENT: vk::ShaderStageFlags = vk::ShaderStageFlags::FRAGMENT;

    fn interval(stage: vk::ShaderStageFlags, offset: u32, size: u32) -> StageInterval {
        StageInterval {
            stage,
            offset,
            size,
        }
    }

    #[test]
    fn merges_identical_ranges_across_stages() {
        let ranges = merge_push_ranges(&[interval(VERTEX, 0, 64), interval(FRAGMENT, 0, 64)], 128)
            .expect("merge");
        assert_eq!(
            ranges,
            vec![PushRange {
                stages: VERTEX | FRAGMENT,
                offset: 0,
                size: 64,
            }]
        );
    }

    #[test]
    fn widens_a_partial_overlap_into_one_range() {
        let ranges = merge_push_ranges(&[interval(VERTEX, 0, 64), interval(FRAGMENT, 0, 32)], 128)
            .expect("merge");
        assert_eq!(
            ranges,
            vec![PushRange {
                stages: VERTEX | FRAGMENT,
                offset: 0,
                size: 64,
            }]
        );
    }

    #[test]
    fn covers_two_intervals_of_one_stage() {
        let ranges = merge_push_ranges(&[interval(VERTEX, 0, 16), interval(VERTEX, 32, 16)], 128)
            .expect("merge");
        assert_eq!(
            ranges,
            vec![PushRange {
                stages: VERTEX,
                offset: 0,
                size: 48,
            }]
        );
    }

    #[test]
    fn keeps_disjoint_ranges() {
        let ranges = merge_push_ranges(&[interval(VERTEX, 0, 16), interval(FRAGMENT, 64, 16)], 128)
            .expect("merge");
        assert_eq!(
            ranges,
            vec![
                PushRange {
                    stages: VERTEX,
                    offset: 0,
                    size: 16,
                },
                PushRange {
                    stages: FRAGMENT,
                    offset: 64,
                    size: 16,
                },
            ]
        );
    }

    #[test]
    fn rejects_a_range_past_the_device_limit() {
        let error = merge_push_ranges(&[interval(VERTEX, 0, 256)], 128).expect_err("limit");
        assert!(error.contains("128"), "{error}");
    }

    #[test]
    fn leaves_binding_holes_and_fills_set_holes() {
        let plan = plan_sets(
            &[
                StageBinding {
                    set: 0,
                    binding: 0,
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    count: 1,
                    runtime_array: false,
                    stages: VERTEX,
                },
                StageBinding {
                    set: 0,
                    binding: 2,
                    ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
                    count: 1,
                    runtime_array: false,
                    stages: FRAGMENT,
                },
                StageBinding {
                    set: 2,
                    binding: 0,
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    count: 1,
                    runtime_array: false,
                    stages: VERTEX | FRAGMENT,
                },
            ],
            4,
        )
        .expect("sets");
        assert_eq!(plan.sets.len(), 3);
        assert!(plan.sets[1].is_empty());
        assert_eq!(plan.sets[0].len(), 2);
        assert_eq!(plan.sets[0][0].binding, 0);
        assert_eq!(plan.sets[0][1].binding, 2);
        assert_eq!(plan.sets[2][0].binding, 0);
    }

    #[test]
    fn rejects_conflicting_bindings() {
        let error = plan_sets(
            &[
                StageBinding {
                    set: 0,
                    binding: 0,
                    ty: vk::DescriptorType::UNIFORM_BUFFER,
                    count: 1,
                    runtime_array: false,
                    stages: VERTEX,
                },
                StageBinding {
                    set: 0,
                    binding: 0,
                    ty: vk::DescriptorType::STORAGE_BUFFER,
                    count: 1,
                    runtime_array: false,
                    stages: FRAGMENT,
                },
            ],
            4,
        )
        .expect_err("conflict");
        assert!(error.contains("binding 0"), "{error}");
    }

    #[test]
    fn triangle_contract_rejects_a_short_range_before_commit() {
        let ranges = [PushRange {
            stages: VERTEX,
            offset: 0,
            size: 32,
        }];
        let contract = PushContract {
            offset: 0,
            size: std::mem::size_of::<[f32; 16]>() as u32,
        };
        assert!(covering(&ranges, contract).is_none());
    }
}
