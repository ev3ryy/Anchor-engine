pub mod cooked;

mod column;
mod components;
mod entity;
mod hierarchy;
mod transform;

pub(crate) use column::Column;
pub use components::{
    AssetId, Camera, Component, ComponentId, ComponentMask, ComponentMaskIter, Draw, Extract,
    ExtractedCamera, ExtractedLight, Light, MaterialRef, MeshRenderer, Model, Name, Primitive,
};
pub use entity::{Entity, Error};
pub use glam::{Mat4, Quat, Vec3};
pub use transform::Transform;

use components::basis;
use entity::{NONE, next_generation};
use hierarchy::Hierarchy;
use transform::local_matrix;

#[cfg(test)]
mod tests;

pub struct Scene {
    generations: Vec<u32>,
    live: Vec<bool>,
    free: Vec<u32>,
    sparse: Vec<u32>,
    dense_to_slot: Vec<u32>,
    local: Vec<Transform>,
    world: Vec<Mat4>,
    parent_dense: Vec<u32>,
    hierarchy: Hierarchy,
    masks: Vec<u32>,
    names: Column<Name>,
    meshes: Column<MeshRenderer>,
    cameras: Column<Camera>,
    lights: Column<Light>,
    structure_dirty: bool,
    transforms_dirty: bool,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            generations: Vec::new(),
            live: Vec::new(),
            free: Vec::new(),
            sparse: Vec::new(),
            dense_to_slot: Vec::new(),
            local: Vec::new(),
            world: Vec::new(),
            parent_dense: Vec::new(),
            hierarchy: Hierarchy::new(),
            masks: Vec::new(),
            names: Column::new(),
            meshes: Column::new(),
            cameras: Column::new(),
            lights: Column::new(),
            structure_dirty: false,
            transforms_dirty: false,
        }
    }

    pub fn spawn(&mut self) -> Entity {
        let slot = self.alloc_slot();
        let entity = self.activate(slot);
        self.hierarchy.link_root(slot);
        entity
    }

    pub fn spawn_child(&mut self, parent: Entity) -> Result<Entity, Error> {
        let parent_slot = self.slot(parent)? as u32;
        let slot = self.alloc_slot();
        let entity = self.activate(slot);
        self.hierarchy.link_child(parent_slot, slot);
        Ok(entity)
    }

    pub fn despawn(&mut self, entity: Entity) -> Result<Vec<Entity>, Error> {
        let root = self.slot(entity)? as u32;
        let slots = self.hierarchy.subtree(root);
        self.hierarchy.unlink(root);
        let mut destroyed = Vec::with_capacity(slots.len());
        for slot in slots {
            let generation = self.generations[slot as usize];
            let entity = Entity::new(slot, generation);
            self.clear_components(entity);
            destroyed.push(entity);
            self.generations[slot as usize] = next_generation(generation);
            self.live[slot as usize] = false;
            self.hierarchy.clear_slot(slot);
            self.swap_remove_dense(slot);
            self.free.push(slot);
        }
        self.structure_dirty = true;
        self.transforms_dirty = true;
        Ok(destroyed)
    }

    pub fn set_parent(&mut self, entity: Entity, parent: Option<Entity>) -> Result<(), Error> {
        let slot = self.slot(entity)? as u32;
        let parent_slot = match parent {
            Some(parent) => Some(self.slot(parent)? as u32),
            None => None,
        };
        if parent_slot == Some(slot)
            || parent_slot.is_some_and(|parent| self.hierarchy.reaches(parent, slot))
        {
            return Err(Error::Cycle);
        }
        let next = parent_slot.unwrap_or(NONE);
        if self.hierarchy.parent_of(slot) == next {
            return Ok(());
        }
        self.hierarchy.unlink(slot);
        match parent_slot {
            Some(parent_slot) => self.hierarchy.link_child(parent_slot, slot),
            None => self.hierarchy.link_root(slot),
        }
        self.structure_dirty = true;
        self.transforms_dirty = true;
        Ok(())
    }

    pub fn parent(&self, entity: Entity) -> Result<Option<Entity>, Error> {
        let slot = self.slot(entity)? as u32;
        let parent = self.hierarchy.parent_of(slot);
        if parent == NONE {
            Ok(None)
        } else {
            Ok(Some(self.entity_at(parent)))
        }
    }

    pub fn children(&self, entity: Entity) -> Result<impl Iterator<Item = Entity> + '_, Error> {
        let slot = self.slot(entity)? as u32;
        Ok(self.entities(self.hierarchy.first_child_of(slot)))
    }

    pub fn roots(&self) -> impl Iterator<Item = Entity> + '_ {
        self.entities(self.hierarchy.first_root())
    }

    pub fn set_local_position(&mut self, entity: Entity, translation: Vec3) -> Result<(), Error> {
        self.local_mut(entity)?.translation = translation;
        Ok(())
    }

    pub fn set_local_rotation(&mut self, entity: Entity, rotation: Quat) -> Result<(), Error> {
        self.local_mut(entity)?.rotation = rotation;
        Ok(())
    }

    pub fn set_local_scale(&mut self, entity: Entity, scale: Vec3) -> Result<(), Error> {
        self.local_mut(entity)?.scale = scale;
        Ok(())
    }

    pub fn local(&self, entity: Entity) -> Result<Transform, Error> {
        let dense = self.dense(entity)?;
        Ok(self.local[dense])
    }

    pub fn world(&self, entity: Entity) -> Result<Mat4, Error> {
        debug_assert!(!self.transforms_dirty);
        let dense = self.dense(entity)?;
        Ok(self.world[dense])
    }

    pub fn extract_into(
        &self,
        out: &mut Extract,
        mut resolve: impl FnMut(AssetId) -> Option<MaterialRef>,
    ) {
        debug_assert!(!self.transforms_dirty);
        out.draws.clear();
        out.cameras.clear();
        out.lights.clear();
        for (entity, mesh) in self.meshes.iter() {
            let Some(world) = self.world_matrix(entity) else {
                continue;
            };
            let Some(material) = resolve(mesh.material) else {
                continue;
            };
            out.draws.push(Draw {
                entity,
                world,
                model: mesh.model,
                material,
            });
        }
        for (entity, camera) in self.cameras.iter() {
            let Some(world) = self.world_matrix(entity) else {
                continue;
            };
            let Some((x_axis, y_axis, z_axis)) = basis(world) else {
                continue;
            };
            out.cameras.push(components::ExtractedCamera {
                entity,
                position: world.w_axis.truncate(),
                x_axis,
                y_axis,
                z_axis,
                fov_y: camera.fov_y,
                near: camera.near,
                far: camera.far,
            });
        }
        for (entity, light) in self.lights.iter() {
            let Light::Directional { color, intensity } = light;
            let Some(world) = self.world_matrix(entity) else {
                continue;
            };
            let Some((_, _, z_axis)) = basis(world) else {
                continue;
            };
            out.lights.push(components::ExtractedLight {
                entity,
                position: world.w_axis.truncate(),
                direction: -z_axis,
                color: *color,
                intensity: *intensity,
            });
        }
    }

    pub fn update(&mut self) {
        if self.structure_dirty {
            self.repack();
            self.transforms_dirty = true;
        }
        if self.transforms_dirty {
            self.propagate();
        }
    }

    pub fn len(&self) -> usize {
        self.dense_to_slot.len()
    }

    pub fn is_empty(&self) -> bool {
        self.dense_to_slot.is_empty()
    }

    fn alloc_slot(&mut self) -> u32 {
        if let Some(slot) = self.free.pop() {
            return slot;
        }
        let slot = self.generations.len() as u32;
        self.generations.push(0);
        self.live.push(false);
        self.sparse.push(NONE);
        self.masks.push(0);
        self.hierarchy.push_slot();
        slot
    }

    fn activate(&mut self, slot: u32) -> Entity {
        let index = slot as usize;
        let generation = next_generation(self.generations[index]);
        self.generations[index] = generation;
        self.live[index] = true;
        self.masks[index] = 0;
        let dense = self.dense_to_slot.len() as u32;
        self.sparse[index] = dense;
        self.dense_to_slot.push(slot);
        self.local.push(Transform::default());
        self.world.push(Mat4::IDENTITY);
        self.parent_dense.push(NONE);
        self.structure_dirty = true;
        self.transforms_dirty = true;
        Entity::new(slot, generation)
    }

    fn swap_remove_dense(&mut self, slot: u32) {
        let dense = self.sparse[slot as usize];
        if dense == NONE {
            return;
        }
        let dense = dense as usize;
        let last = self.dense_to_slot.len() - 1;
        if dense != last {
            let moved = self.dense_to_slot[last];
            self.dense_to_slot[dense] = moved;
            self.local[dense] = self.local[last];
            self.world[dense] = self.world[last];
            self.parent_dense[dense] = self.parent_dense[last];
            self.sparse[moved as usize] = dense as u32;
        }
        self.dense_to_slot.pop();
        self.local.pop();
        self.world.pop();
        self.parent_dense.pop();
        self.sparse[slot as usize] = NONE;
    }

    fn repack(&mut self) {
        let count = self.dense_to_slot.len();
        let mut dense_to_slot = Vec::with_capacity(count);
        let mut local = Vec::with_capacity(count);
        let mut parent_dense = Vec::with_capacity(count);
        let mut stack = Vec::new();
        self.hierarchy
            .push_reverse(self.hierarchy.last_root(), &mut stack);
        while let Some(slot) = stack.pop() {
            let index = slot as usize;
            let old_dense = self.sparse[index];
            debug_assert!(old_dense != NONE);
            let parent_slot = self.hierarchy.parent_of(slot);
            let parent = if parent_slot == NONE {
                NONE
            } else {
                self.sparse[parent_slot as usize]
            };
            let new_dense = dense_to_slot.len() as u32;
            self.sparse[index] = new_dense;
            dense_to_slot.push(slot);
            local.push(self.local[old_dense as usize]);
            parent_dense.push(parent);
            self.hierarchy
                .push_reverse(self.hierarchy.last_child_of(slot), &mut stack);
        }
        debug_assert_eq!(dense_to_slot.len(), count);
        self.dense_to_slot = dense_to_slot;
        self.local = local;
        self.parent_dense = parent_dense;
        self.world.resize(count, Mat4::IDENTITY);
        self.structure_dirty = false;
    }

    fn propagate(&mut self) {
        for index in 0..self.local.len() {
            let local = local_matrix(&self.local[index]);
            let parent = self.parent_dense[index];
            self.world[index] = if parent == NONE {
                local
            } else {
                debug_assert!(parent < index as u32);
                self.world[parent as usize] * local
            };
        }
        self.transforms_dirty = false;
    }

    fn slot(&self, entity: Entity) -> Result<usize, Error> {
        let index = entity.index() as usize;
        if self.generations.get(index).copied() != Some(entity.generation())
            || !self.live.get(index).copied().unwrap_or(false)
        {
            return Err(Error::StaleEntity);
        }
        Ok(index)
    }

    fn dense(&self, entity: Entity) -> Result<usize, Error> {
        let slot = self.slot(entity)?;
        let dense = self.sparse[slot];
        if dense == NONE {
            return Err(Error::StaleEntity);
        }
        Ok(dense as usize)
    }

    fn local_mut(&mut self, entity: Entity) -> Result<&mut Transform, Error> {
        let dense = self.dense(entity)?;
        self.transforms_dirty = true;
        Ok(&mut self.local[dense])
    }

    fn clear_components(&mut self, entity: Entity) {
        let slot = entity.index() as usize;
        let mask = ComponentMask(self.masks[slot]);
        for id in mask.iter() {
            match id {
                ComponentId::Name => {
                    self.names.remove(entity);
                }
                ComponentId::Mesh => {
                    self.meshes.remove(entity);
                }
                ComponentId::Camera => {
                    self.cameras.remove(entity);
                }
                ComponentId::Light => {
                    self.lights.remove(entity);
                }
            }
        }
        self.masks[slot] = 0;
    }

    fn world_matrix(&self, entity: Entity) -> Option<Mat4> {
        let slot = entity.index() as usize;
        if self.generations.get(slot).copied() != Some(entity.generation()) {
            return None;
        }
        let dense = *self.sparse.get(slot)?;
        if dense == NONE {
            return None;
        }
        self.world.get(dense as usize).copied()
    }

    fn entity_at(&self, slot: u32) -> Entity {
        Entity::new(slot, self.generations[slot as usize])
    }

    fn entities(&self, next: u32) -> Entities<'_> {
        Entities {
            generations: &self.generations,
            next_sibling: self.hierarchy.next_siblings(),
            next,
        }
    }
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

struct Entities<'a> {
    generations: &'a [u32],
    next_sibling: &'a [u32],
    next: u32,
}

impl Iterator for Entities<'_> {
    type Item = Entity;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next == NONE {
            return None;
        }
        let slot = self.next;
        let entity = Entity::new(slot, self.generations[slot as usize]);
        self.next = self.next_sibling[slot as usize];
        Some(entity)
    }
}
