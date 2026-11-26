use std::fmt;

use glam::{Mat4, Vec3};

use crate::{Entity, Error, Scene};

const AXIS_EPSILON: f32 = 1.0e-8;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Mesh {
    Cube,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MaterialRef {
    pub index: u32,
    pub generation: u32,
}

#[repr(C)]
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, bytemuck::Pod, bytemuck::Zeroable,
)]
pub struct AssetId(pub [u8; 16]);

impl AssetId {
    pub fn parse(text: &str) -> Option<Self> {
        let hyphenated = text.len() == 36;
        if hyphenated {
            let bytes = text.as_bytes();
            if bytes[8] != b'-' || bytes[13] != b'-' || bytes[18] != b'-' || bytes[23] != b'-' {
                return None;
            }
        }
        let hex: Vec<u8> = text.bytes().filter(|byte| *byte != b'-').collect();
        if hex.len() != 32 {
            return None;
        }
        let mut bytes = [0; 16];
        for index in 0..16 {
            let pair = std::str::from_utf8(&hex[index * 2..index * 2 + 2]).ok()?;
            bytes[index] = u8::from_str_radix(pair, 16).ok()?;
        }
        Some(Self(bytes))
    }

    pub fn canonical(self) -> String {
        format!(
            "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            self.0[0],
            self.0[1],
            self.0[2],
            self.0[3],
            self.0[4],
            self.0[5],
            self.0[6],
            self.0[7],
            self.0[8],
            self.0[9],
            self.0[10],
            self.0[11],
            self.0[12],
            self.0[13],
            self.0[14],
            self.0[15],
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeshRenderer {
    pub mesh: Mesh,
    pub material: AssetId,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Camera {
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Light {
    Directional { color: Vec3, intensity: f32 },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Name {
    value: String,
}

impl Name {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }

    pub fn as_str(&self) -> &str {
        &self.value
    }
}

impl From<&str> for Name {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for Name {
    fn from(value: String) -> Self {
        Self { value }
    }
}

impl AsRef<str> for Name {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Name {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl PartialEq<str> for Name {
    fn eq(&self, other: &str) -> bool {
        self.value == other
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ComponentId {
    Name = 1 << 0,
    Mesh = 1 << 1,
    Camera = 1 << 2,
    Light = 1 << 3,
}

const COMPONENT_IDS: [ComponentId; 4] = [
    ComponentId::Name,
    ComponentId::Mesh,
    ComponentId::Camera,
    ComponentId::Light,
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ComponentMask(pub(crate) u32);

impl ComponentMask {
    pub const EMPTY: Self = Self(0);

    pub const fn contains(self, id: ComponentId) -> bool {
        self.0 & (id as u32) != 0
    }

    pub const fn iter(self) -> ComponentMaskIter {
        ComponentMaskIter {
            mask: self.0,
            bit: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ComponentMaskIter {
    mask: u32,
    bit: usize,
}

impl Iterator for ComponentMaskIter {
    type Item = ComponentId;

    fn next(&mut self) -> Option<Self::Item> {
        while self.bit < COMPONENT_IDS.len() {
            let id = COMPONENT_IDS[self.bit];
            self.bit += 1;
            if self.mask & (id as u32) != 0 {
                return Some(id);
            }
        }
        None
    }
}

mod sealed {
    use super::{Entity, Scene};

    pub trait Sealed {}

    pub trait Ops: Sealed + Sized {
        fn insert(scene: &mut Scene, entity: Entity, value: Self) -> Option<Self>;
        fn get<'a>(scene: &'a Scene, entity: Entity) -> Option<&'a Self>;
        fn get_mut<'a>(scene: &'a mut Scene, entity: Entity) -> Option<&'a mut Self>;
        fn remove(scene: &mut Scene, entity: Entity) -> Option<Self>;
        fn iter<'a>(scene: &'a Scene) -> impl Iterator<Item = (Entity, &'a Self)>
        where
            Self: 'a;
        fn iter_mut<'a>(scene: &'a mut Scene) -> impl Iterator<Item = (Entity, &'a mut Self)>
        where
            Self: 'a;
    }
}

pub trait Component: sealed::Ops {
    const ID: ComponentId;
    const MASK: ComponentMask;
}

impl sealed::Sealed for Name {}

impl sealed::Ops for Name {
    fn insert(scene: &mut Scene, entity: Entity, value: Self) -> Option<Self> {
        scene.names.insert(entity, value)
    }

    fn get<'a>(scene: &'a Scene, entity: Entity) -> Option<&'a Self> {
        scene.names.get(entity)
    }

    fn get_mut<'a>(scene: &'a mut Scene, entity: Entity) -> Option<&'a mut Self> {
        scene.names.get_mut(entity)
    }

    fn remove(scene: &mut Scene, entity: Entity) -> Option<Self> {
        scene.names.remove(entity)
    }

    fn iter<'a>(scene: &'a Scene) -> impl Iterator<Item = (Entity, &'a Self)>
    where
        Self: 'a,
    {
        scene.names.iter()
    }

    fn iter_mut<'a>(scene: &'a mut Scene) -> impl Iterator<Item = (Entity, &'a mut Self)>
    where
        Self: 'a,
    {
        scene.names.iter_mut()
    }
}

impl Component for Name {
    const ID: ComponentId = ComponentId::Name;
    const MASK: ComponentMask = ComponentMask(ComponentId::Name as u32);
}

impl sealed::Sealed for MeshRenderer {}

impl sealed::Ops for MeshRenderer {
    fn insert(scene: &mut Scene, entity: Entity, value: Self) -> Option<Self> {
        scene.meshes.insert(entity, value)
    }

    fn get<'a>(scene: &'a Scene, entity: Entity) -> Option<&'a Self> {
        scene.meshes.get(entity)
    }

    fn get_mut<'a>(scene: &'a mut Scene, entity: Entity) -> Option<&'a mut Self> {
        scene.meshes.get_mut(entity)
    }

    fn remove(scene: &mut Scene, entity: Entity) -> Option<Self> {
        scene.meshes.remove(entity)
    }

    fn iter<'a>(scene: &'a Scene) -> impl Iterator<Item = (Entity, &'a Self)>
    where
        Self: 'a,
    {
        scene.meshes.iter()
    }

    fn iter_mut<'a>(scene: &'a mut Scene) -> impl Iterator<Item = (Entity, &'a mut Self)>
    where
        Self: 'a,
    {
        scene.meshes.iter_mut()
    }
}

impl Component for MeshRenderer {
    const ID: ComponentId = ComponentId::Mesh;
    const MASK: ComponentMask = ComponentMask(ComponentId::Mesh as u32);
}

impl sealed::Sealed for Camera {}

impl sealed::Ops for Camera {
    fn insert(scene: &mut Scene, entity: Entity, value: Self) -> Option<Self> {
        scene.cameras.insert(entity, value)
    }

    fn get<'a>(scene: &'a Scene, entity: Entity) -> Option<&'a Self> {
        scene.cameras.get(entity)
    }

    fn get_mut<'a>(scene: &'a mut Scene, entity: Entity) -> Option<&'a mut Self> {
        scene.cameras.get_mut(entity)
    }

    fn remove(scene: &mut Scene, entity: Entity) -> Option<Self> {
        scene.cameras.remove(entity)
    }

    fn iter<'a>(scene: &'a Scene) -> impl Iterator<Item = (Entity, &'a Self)>
    where
        Self: 'a,
    {
        scene.cameras.iter()
    }

    fn iter_mut<'a>(scene: &'a mut Scene) -> impl Iterator<Item = (Entity, &'a mut Self)>
    where
        Self: 'a,
    {
        scene.cameras.iter_mut()
    }
}

impl Component for Camera {
    const ID: ComponentId = ComponentId::Camera;
    const MASK: ComponentMask = ComponentMask(ComponentId::Camera as u32);
}

impl sealed::Sealed for Light {}

impl sealed::Ops for Light {
    fn insert(scene: &mut Scene, entity: Entity, value: Self) -> Option<Self> {
        scene.lights.insert(entity, value)
    }

    fn get<'a>(scene: &'a Scene, entity: Entity) -> Option<&'a Self> {
        scene.lights.get(entity)
    }

    fn get_mut<'a>(scene: &'a mut Scene, entity: Entity) -> Option<&'a mut Self> {
        scene.lights.get_mut(entity)
    }

    fn remove(scene: &mut Scene, entity: Entity) -> Option<Self> {
        scene.lights.remove(entity)
    }

    fn iter<'a>(scene: &'a Scene) -> impl Iterator<Item = (Entity, &'a Self)>
    where
        Self: 'a,
    {
        scene.lights.iter()
    }

    fn iter_mut<'a>(scene: &'a mut Scene) -> impl Iterator<Item = (Entity, &'a mut Self)>
    where
        Self: 'a,
    {
        scene.lights.iter_mut()
    }
}

impl Component for Light {
    const ID: ComponentId = ComponentId::Light;
    const MASK: ComponentMask = ComponentMask(ComponentId::Light as u32);
}

impl Scene {
    pub fn insert<T: Component>(&mut self, entity: Entity, value: T) -> Result<Option<T>, Error> {
        let slot = self.slot(entity)?;
        let previous = <T as sealed::Ops>::insert(self, entity, value);
        self.masks[slot] |= T::MASK.0;
        Ok(previous)
    }

    pub fn get<T: Component>(&self, entity: Entity) -> Result<Option<&T>, Error> {
        self.slot(entity)?;
        Ok(<T as sealed::Ops>::get(self, entity))
    }

    pub fn get_mut<T: Component>(&mut self, entity: Entity) -> Result<Option<&mut T>, Error> {
        self.slot(entity)?;
        Ok(<T as sealed::Ops>::get_mut(self, entity))
    }

    pub fn remove<T: Component>(&mut self, entity: Entity) -> Result<Option<T>, Error> {
        let slot = self.slot(entity)?;
        let value = <T as sealed::Ops>::remove(self, entity);
        if value.is_some() {
            self.masks[slot] &= !T::MASK.0;
        }
        Ok(value)
    }

    pub fn iter<'a, T: Component + 'a>(&'a self) -> impl Iterator<Item = (Entity, &'a T)> + 'a {
        <T as sealed::Ops>::iter(self)
    }

    pub fn iter_mut<'a, T: Component + 'a>(
        &'a mut self,
    ) -> impl Iterator<Item = (Entity, &'a mut T)> + 'a {
        <T as sealed::Ops>::iter_mut(self)
    }

    pub fn has<T: Component>(&self, entity: Entity) -> Result<bool, Error> {
        let slot = self.slot(entity)?;
        Ok(self.masks[slot] & T::MASK.0 != 0)
    }

    pub fn components(&self, entity: Entity) -> Result<ComponentMask, Error> {
        let slot = self.slot(entity)?;
        Ok(ComponentMask(self.masks[slot]))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Draw {
    pub entity: Entity,
    pub world: Mat4,
    pub mesh: Mesh,
    pub material: MaterialRef,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedCamera {
    pub entity: Entity,
    pub position: Vec3,
    pub x_axis: Vec3,
    pub y_axis: Vec3,
    pub z_axis: Vec3,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExtractedLight {
    pub entity: Entity,
    pub position: Vec3,
    pub direction: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Extract {
    pub draws: Vec<Draw>,
    pub cameras: Vec<ExtractedCamera>,
    pub lights: Vec<ExtractedLight>,
}

pub(crate) fn basis(world: Mat4) -> Option<(Vec3, Vec3, Vec3)> {
    let x = world.x_axis.truncate();
    let y = world.y_axis.truncate();
    let z = world.z_axis.truncate();
    if x.length_squared() < AXIS_EPSILON
        || y.length_squared() < AXIS_EPSILON
        || z.length_squared() < AXIS_EPSILON
    {
        return None;
    }
    let z_axis = z.normalize();
    let mut y_axis = y - z_axis * y.dot(z_axis);
    if y_axis.length_squared() < AXIS_EPSILON {
        return None;
    }
    y_axis = y_axis.normalize();
    let x_axis = y_axis.cross(z_axis);
    Some((x_axis, y_axis, z_axis))
}
