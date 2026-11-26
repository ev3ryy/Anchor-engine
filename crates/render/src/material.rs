use crate::Error;
use crate::texture::{SamplerId, TextureId};

pub const MATERIAL_CAPACITY: u32 = 256;
pub const PARAM_STRIDE: usize = 256;
pub const HEADER_STRIDE: usize = 80;
pub const TYPE_UNLIT: u32 = 1;

#[repr(C)]
#[derive(Clone, Copy)]
struct MaterialRef {
    type_id: u32,
    flags: u32,
    param_offset: u32,
    param_size: u32,
    textures: [u32; 8],
    samplers: [u32; 8],
}

const _: () = assert!(std::mem::size_of::<MaterialRef>() == HEADER_STRIDE);
const _: () = assert!(std::mem::size_of::<FramePush>() == 80);

#[repr(C)]
#[derive(Clone, Copy)]
pub struct FramePush {
    pub view_proj: [f32; 16],
    pub material_index: u32,
    pub instance_base: u32,
    pub pad1: u32,
    pub pad2: u32,
}

const _: () = assert!(std::mem::offset_of!(FramePush, material_index) == 64);
const _: () = assert!(std::mem::offset_of!(FramePush, instance_base) == 68);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MaterialId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum MaterialDesc {
    Unlit {
        color: [f32; 4],
        albedo: TextureId,
        sampler: SamplerId,
    },
}

struct Slot {
    generation: u32,
    live: bool,
    dirty: bool,
    header: MaterialRef,
    params: [u8; PARAM_STRIDE],
}

pub struct MaterialStore {
    slots: Vec<Slot>,
    free: Vec<u32>,
}

impl MaterialStore {
    pub fn new(capacity: u32) -> Self {
        let slots = (0..capacity)
            .map(|_| Slot {
                generation: 0,
                live: false,
                dirty: false,
                header: MaterialRef {
                    type_id: 0,
                    flags: 0,
                    param_offset: 0,
                    param_size: 0,
                    textures: [0; 8],
                    samplers: [0; 8],
                },
                params: [0; PARAM_STRIDE],
            })
            .collect();
        let free = (0..capacity).rev().collect();
        Self { slots, free }
    }

    pub fn create(
        &mut self,
        albedo: u32,
        sampler: SamplerId,
        color: [f32; 4],
    ) -> Result<MaterialId, Error> {
        let index = self.free.pop().ok_or(Error::OutOfMaterials)?;
        let slot = &mut self.slots[index as usize];
        slot.generation = next_generation(slot.generation);
        slot.live = true;
        write_unlit(slot, index, albedo, sampler, color);
        Ok(MaterialId {
            index,
            generation: slot.generation,
        })
    }

    pub fn update(
        &mut self,
        id: MaterialId,
        albedo: u32,
        sampler: SamplerId,
        color: [f32; 4],
    ) -> Result<(), Error> {
        let slot = self.slot_mut(id)?;
        write_unlit(slot, id.index, albedo, sampler, color);
        Ok(())
    }

    pub fn free(&mut self, id: MaterialId) -> Result<(), Error> {
        {
            let slot = self.slot_mut(id)?;
            slot.live = false;
            slot.generation = next_generation(slot.generation);
            slot.header = MaterialRef {
                type_id: 0,
                flags: 0,
                param_offset: id.index * PARAM_STRIDE as u32,
                param_size: 0,
                textures: [0; 8],
                samplers: [0; 8],
            };
            slot.params = [0; PARAM_STRIDE];
            slot.dirty = true;
        }
        self.free.push(id.index);
        Ok(())
    }

    pub fn index(&self, id: MaterialId) -> Result<u32, Error> {
        let slot = self
            .slots
            .get(id.index as usize)
            .ok_or(Error::StaleMaterial)?;
        if !slot.live || slot.generation != id.generation {
            return Err(Error::StaleMaterial);
        }
        Ok(id.index)
    }

    pub fn flush(&mut self, headers: *mut u8, params: *mut u8) {
        for (index, slot) in self.slots.iter_mut().enumerate() {
            if !slot.dirty {
                continue;
            }
            unsafe {
                std::ptr::copy_nonoverlapping(
                    (&slot.header as *const MaterialRef).cast::<u8>(),
                    headers.add(index * HEADER_STRIDE),
                    HEADER_STRIDE,
                );
                std::ptr::copy_nonoverlapping(
                    slot.params.as_ptr(),
                    params.add(index * PARAM_STRIDE),
                    PARAM_STRIDE,
                );
            }
            slot.dirty = false;
        }
    }

    fn slot_mut(&mut self, id: MaterialId) -> Result<&mut Slot, Error> {
        let slot = self
            .slots
            .get_mut(id.index as usize)
            .ok_or(Error::StaleMaterial)?;
        if !slot.live || slot.generation != id.generation {
            return Err(Error::StaleMaterial);
        }
        Ok(slot)
    }
}

fn write_unlit(slot: &mut Slot, index: u32, albedo: u32, sampler: SamplerId, color: [f32; 4]) {
    let mut textures = [0; 8];
    let mut samplers = [0; 8];
    textures[0] = albedo;
    samplers[0] = sampler as u32;
    slot.header = MaterialRef {
        type_id: TYPE_UNLIT,
        flags: 0,
        param_offset: index * PARAM_STRIDE as u32,
        param_size: 16,
        textures,
        samplers,
    };
    slot.params = [0; PARAM_STRIDE];
    write_color(&mut slot.params, color);
    slot.dirty = true;
}

fn write_color(params: &mut [u8], color: [f32; 4]) {
    for (index, channel) in color.into_iter().enumerate() {
        let start = index * 4;
        params[start..start + 4].copy_from_slice(&channel.to_le_bytes());
    }
}

fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

#[cfg(test)]
mod tests {
    use super::{HEADER_STRIDE, MaterialRef, MaterialStore, PARAM_STRIDE, TYPE_UNLIT, write_color};
    use crate::texture::SamplerId;

    #[test]
    fn frame_push_tail_starts_at_byte_64() {
        assert_eq!(std::mem::size_of::<super::FramePush>(), 80);
        assert_eq!(std::mem::offset_of!(super::FramePush, material_index), 64);
        assert_eq!(std::mem::offset_of!(super::FramePush, instance_base), 68);
    }

    #[test]
    fn unlit_color_is_sixteen_bytes_at_the_slot_origin() {
        let mut params = [0u8; PARAM_STRIDE];
        write_color(&mut params, [1.0, 0.5, 0.25, 1.0]);
        assert_eq!(&params[0..4], &1.0f32.to_le_bytes());
        assert_eq!(&params[4..8], &0.5f32.to_le_bytes());
        assert_eq!(&params[8..12], &0.25f32.to_le_bytes());
        assert_eq!(&params[12..16], &1.0f32.to_le_bytes());
        assert!(params[16..].iter().all(|byte| *byte == 0));
    }

    #[test]
    fn create_marks_only_the_cpu_slot_dirty() {
        let mut store = MaterialStore::new(4);
        let id = store
            .create(3, SamplerId::LinearRepeat, [1.0, 1.0, 1.0, 1.0])
            .expect("material");
        assert_eq!(store.index(id).expect("index"), id.index);
        let slot = &store.slots[id.index as usize];
        assert!(slot.dirty);
        assert_eq!(slot.header.type_id, TYPE_UNLIT);
        assert_eq!(slot.header.textures[0], 3);
        assert_eq!(slot.header.param_offset, id.index * PARAM_STRIDE as u32);
        assert_eq!(slot.header.param_size, 16);
        assert_eq!(std::mem::size_of::<MaterialRef>(), HEADER_STRIDE);
    }
}
