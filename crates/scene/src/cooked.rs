use std::mem::{align_of, size_of};

use bytemuck::{Pod, Zeroable, bytes_of, try_cast_slice, try_from_bytes};
use glam::Mat4;

use crate::{
    AssetId, Camera, ComponentId, Light, MeshRenderer, Model, Name, Primitive, Quat, Scene,
    Transform, Vec3,
};

pub const ALIGN: usize = 64;
pub const HEADER_SIZE: usize = 128;
pub const NONE: u32 = u32::MAX;
pub const MAGIC: u32 = u32::from_le_bytes(*b"SCN1");
pub const VERSION: u32 = 1;
pub const MESH_CUBE: u32 = 0;
pub const MESH_SPHERE: u32 = 1;
pub const MESH_CYLINDER: u32 = 2;
pub const MESH_CAPSULE: u32 = 3;
pub const MESH_PLANE: u32 = 4;
pub const MESH_CONE: u32 = 5;
pub const MESH_WEDGE: u32 = 6;
pub const LIGHT_DIRECTIONAL: u32 = 0;

const KNOWN_MASK: u32 = ComponentId::Name as u32
    | ComponentId::Mesh as u32
    | ComponentId::Camera as u32
    | ComponentId::Light as u32;

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct Header {
    pub magic: u32,
    pub version: u32,
    pub entity_count: u32,
    pub mesh_count: u32,
    pub camera_count: u32,
    pub light_count: u32,
    pub string_count: u32,
    pub string_bytes: u32,
    pub first_root: u32,
    pub entities: u32,
    pub locals: u32,
    pub worlds: u32,
    pub meshes: u32,
    pub cameras: u32,
    pub lights: u32,
    pub strings: u32,
    pub string_data: u32,
    pub _pad0: [u8; 32],
    pub _pad1: [u8; 16],
    pub _pad2: [u8; 8],
    pub _pad3: [u8; 4],
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct CookedEntity {
    pub parent: u32,
    pub first_child: u32,
    pub next_sibling: u32,
    pub name: u32,
    pub mask: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct CookedTransform {
    pub translation: [f32; 4],
    pub rotation: [f32; 4],
    pub scale: [f32; 4],
}

impl CookedTransform {
    pub fn from_transform(transform: Transform) -> Self {
        let translation = transform.translation;
        let rotation = transform.rotation;
        let scale = transform.scale;
        Self {
            translation: [translation.x, translation.y, translation.z, 0.0],
            rotation: [rotation.x, rotation.y, rotation.z, rotation.w],
            scale: [scale.x, scale.y, scale.z, 1.0],
        }
    }

    pub fn to_transform(self) -> Transform {
        Transform {
            translation: Vec3::new(
                self.translation[0],
                self.translation[1],
                self.translation[2],
            ),
            rotation: Quat::from_xyzw(
                self.rotation[0],
                self.rotation[1],
                self.rotation[2],
                self.rotation[3],
            ),
            scale: Vec3::new(self.scale[0], self.scale[1], self.scale[2]),
        }
    }
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct CookedMesh {
    pub entity: u32,
    pub kind: u32,
    pub _pad: [u8; 8],
    pub material: [u8; 16],
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct CookedCamera {
    pub entity: u32,
    pub _pad0: u32,
    pub fov_y: f32,
    pub near: f32,
    pub far: f32,
    pub _pad1: [f32; 3],
}

#[repr(C, align(16))]
#[derive(Clone, Copy, Debug, PartialEq, Pod, Zeroable)]
pub struct CookedLight {
    pub entity: u32,
    pub kind: u32,
    pub color: [f32; 3],
    pub intensity: f32,
    pub _pad: [f32; 2],
}

#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Pod, Zeroable)]
pub struct CookedStr {
    pub offset: u32,
    pub len: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Misaligned,
    Magic,
    Version,
    Truncated,
    Overflow,
    Overlap,
    Range,
    Hierarchy,
    Mask,
    Column,
    Kind,
    Utf8,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Misaligned => write!(f, "cooked scene is misaligned"),
            Self::Magic => write!(f, "cooked scene magic"),
            Self::Version => write!(f, "cooked scene version"),
            Self::Truncated => write!(f, "cooked scene is truncated"),
            Self::Overflow => write!(f, "cooked scene section overflows"),
            Self::Overlap => write!(f, "cooked scene sections overlap"),
            Self::Range => write!(f, "cooked scene index is out of range"),
            Self::Hierarchy => write!(f, "cooked scene hierarchy"),
            Self::Mask => write!(f, "cooked scene component mask"),
            Self::Column => write!(f, "cooked scene column"),
            Self::Kind => write!(f, "cooked scene component kind"),
            Self::Utf8 => write!(f, "cooked scene string is not utf-8"),
        }
    }
}

impl std::error::Error for Error {}

const _: () = assert!(size_of::<Header>() == HEADER_SIZE);
const _: () = assert!(align_of::<Header>() <= ALIGN);
const _: () = assert!(size_of::<CookedEntity>() == 32 && align_of::<CookedEntity>() == 16);
const _: () = assert!(size_of::<CookedTransform>() == 48 && align_of::<CookedTransform>() == 16);
const _: () = assert!(Primitive::Cube.kind() == MESH_CUBE);
const _: () = assert!(Primitive::Sphere.kind() == MESH_SPHERE);
const _: () = assert!(Primitive::Cylinder.kind() == MESH_CYLINDER);
const _: () = assert!(Primitive::Capsule.kind() == MESH_CAPSULE);
const _: () = assert!(Primitive::Plane.kind() == MESH_PLANE);
const _: () = assert!(Primitive::Cone.kind() == MESH_CONE);
const _: () = assert!(Primitive::Wedge.kind() == MESH_WEDGE);
const _: () = assert!(size_of::<CookedMesh>() == 32 && align_of::<CookedMesh>() == 16);
const _: () = assert!(std::mem::offset_of!(CookedMesh, material) == 16);
const _: () = assert!(size_of::<CookedCamera>() == 32 && align_of::<CookedCamera>() == 16);
const _: () = assert!(size_of::<CookedLight>() == 32 && align_of::<CookedLight>() == 16);
const _: () = assert!(size_of::<CookedStr>() == 8 && align_of::<CookedStr>() == 4);
const _: () = assert!(size_of::<Mat4>() == 64 && align_of::<Mat4>() == 16);

#[repr(C, align(64))]
#[derive(Clone, Copy)]
struct Chunk([u8; ALIGN]);

pub struct AlignedBytes {
    chunks: Vec<Chunk>,
    len: usize,
}

impl AlignedBytes {
    pub fn from_slice(bytes: &[u8]) -> Self {
        let mut aligned = Self::with_len(bytes.len());
        aligned.as_mut_slice().copy_from_slice(bytes);
        aligned
    }

    pub fn with_len(len: usize) -> Self {
        let chunks = len.div_ceil(ALIGN);
        Self {
            chunks: vec![Chunk([0; ALIGN]); chunks],
            len,
        }
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.storage()[..self.len]
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        let len = self.len;
        &mut self.storage_mut()[..len]
    }

    fn storage(&self) -> &[u8] {
        // `Chunk` is 64-aligned, so the allocation and this byte view share that alignment.
        unsafe {
            std::slice::from_raw_parts(self.chunks.as_ptr().cast(), self.chunks.len() * ALIGN)
        }
    }

    fn storage_mut(&mut self) -> &mut [u8] {
        unsafe {
            std::slice::from_raw_parts_mut(
                self.chunks.as_mut_ptr().cast(),
                self.chunks.len() * ALIGN,
            )
        }
    }
}

#[derive(Clone, Copy)]
pub struct Flat<'a> {
    pub first_root: u32,
    pub entities: &'a [CookedEntity],
    pub locals: &'a [CookedTransform],
    pub worlds: &'a [Mat4],
    pub meshes: &'a [CookedMesh],
    pub cameras: &'a [CookedCamera],
    pub lights: &'a [CookedLight],
    pub strings: &'a [CookedStr],
    pub string_bytes: &'a [u8],
}

pub fn write(flat: &Flat<'_>) -> Result<AlignedBytes, Error> {
    if flat.locals.len() != flat.entities.len() || flat.worlds.len() != flat.entities.len() {
        return Err(Error::Range);
    }
    let mut buf = vec![0u8; HEADER_SIZE];
    let entities = write_section(&mut buf, align_of::<CookedEntity>(), flat.entities)?;
    let locals = write_section(&mut buf, align_of::<CookedTransform>(), flat.locals)?;
    let worlds = write_section(&mut buf, align_of::<Mat4>(), flat.worlds)?;
    let meshes = write_section(&mut buf, align_of::<CookedMesh>(), flat.meshes)?;
    let cameras = write_section(&mut buf, align_of::<CookedCamera>(), flat.cameras)?;
    let lights = write_section(&mut buf, align_of::<CookedLight>(), flat.lights)?;
    let strings = write_section(&mut buf, align_of::<CookedStr>(), flat.strings)?;
    let string_data = write_raw(&mut buf, flat.string_bytes)?;
    let header = Header {
        magic: MAGIC,
        version: VERSION,
        entity_count: count(flat.entities)?,
        mesh_count: count(flat.meshes)?,
        camera_count: count(flat.cameras)?,
        light_count: count(flat.lights)?,
        string_count: count(flat.strings)?,
        string_bytes: count(flat.string_bytes)?,
        first_root: flat.first_root,
        entities,
        locals,
        worlds,
        meshes,
        cameras,
        lights,
        strings,
        string_data,
        _pad0: [0; 32],
        _pad1: [0; 16],
        _pad2: [0; 8],
        _pad3: [0; 4],
    };
    buf[..HEADER_SIZE].copy_from_slice(bytes_of(&header));
    Ok(AlignedBytes::from_slice(&buf))
}

fn write_section<T: Pod>(buf: &mut Vec<u8>, align: usize, items: &[T]) -> Result<u32, Error> {
    if items.is_empty() {
        return Ok(0);
    }
    pad(buf, align);
    let offset = count_usize(buf.len())?;
    buf.extend_from_slice(try_cast_slice(items).map_err(|_| Error::Misaligned)?);
    Ok(offset)
}

fn write_raw(buf: &mut Vec<u8>, bytes: &[u8]) -> Result<u32, Error> {
    if bytes.is_empty() {
        return Ok(0);
    }
    let offset = count_usize(buf.len())?;
    buf.extend_from_slice(bytes);
    Ok(offset)
}

fn pad(buf: &mut Vec<u8>, align: usize) {
    let extra = (align - (buf.len() % align)) % align;
    buf.extend(std::iter::repeat_n(0, extra));
}

fn count<T>(items: &[T]) -> Result<u32, Error> {
    count_usize(items.len())
}

fn count_usize(len: usize) -> Result<u32, Error> {
    u32::try_from(len).map_err(|_| Error::Overflow)
}

pub struct CookedScene<'a> {
    entities: &'a [CookedEntity],
    locals: &'a [CookedTransform],
    worlds: &'a [Mat4],
    meshes: &'a [CookedMesh],
    cameras: &'a [CookedCamera],
    lights: &'a [CookedLight],
    strings: &'a [CookedStr],
    string_bytes: &'a [u8],
}

impl<'a> CookedScene<'a> {
    pub fn validate(bytes: &'a [u8]) -> Result<Self, Error> {
        if bytes.len() < HEADER_SIZE {
            return Err(Error::Truncated);
        }
        if bytes.as_ptr() as usize % ALIGN != 0 {
            return Err(Error::Misaligned);
        }
        let header: &Header =
            try_from_bytes(&bytes[..HEADER_SIZE]).map_err(|_| Error::Misaligned)?;
        if header.magic != MAGIC {
            return Err(Error::Magic);
        }
        if header.version != VERSION {
            return Err(Error::Version);
        }
        let len = bytes.len() as u64;
        let mut spans = vec![(0u64, HEADER_SIZE as u64)];
        let entities =
            section::<CookedEntity>(bytes, header.entities, header.entity_count, &mut spans, len)?;
        let locals =
            section::<CookedTransform>(bytes, header.locals, header.entity_count, &mut spans, len)?;
        let worlds = section::<Mat4>(bytes, header.worlds, header.entity_count, &mut spans, len)?;
        let meshes =
            section::<CookedMesh>(bytes, header.meshes, header.mesh_count, &mut spans, len)?;
        let cameras =
            section::<CookedCamera>(bytes, header.cameras, header.camera_count, &mut spans, len)?;
        let lights =
            section::<CookedLight>(bytes, header.lights, header.light_count, &mut spans, len)?;
        let strings =
            section::<CookedStr>(bytes, header.strings, header.string_count, &mut spans, len)?;
        let string_bytes = raw_section(
            bytes,
            header.string_data,
            header.string_bytes,
            &mut spans,
            len,
        )?;
        disjoint(&mut spans)?;
        link_index(header.first_root, header.entity_count)?;
        let mut masks = vec![0u32; entities.len()];
        for (index, entity) in entities.iter().enumerate() {
            link_index(entity.parent, header.entity_count)?;
            link_index(entity.first_child, header.entity_count)?;
            link_index(entity.next_sibling, header.entity_count)?;
            if entity.parent != NONE && entity.parent >= index as u32 {
                return Err(Error::Hierarchy);
            }
            if entity.name == NONE {
                continue;
            }
            if entity.name >= header.string_count {
                return Err(Error::Range);
            }
            masks[index] |= ComponentId::Name as u32;
        }
        column(
            meshes,
            header.entity_count,
            ComponentId::Mesh as u32,
            &mut masks,
        )?;
        column(
            cameras,
            header.entity_count,
            ComponentId::Camera as u32,
            &mut masks,
        )?;
        column(
            lights,
            header.entity_count,
            ComponentId::Light as u32,
            &mut masks,
        )?;
        for mesh in meshes {
            if Primitive::from_kind(mesh.kind).is_none() {
                return Err(Error::Kind);
            }
        }
        for light in lights {
            if light.kind != LIGHT_DIRECTIONAL {
                return Err(Error::Kind);
            }
        }
        for (index, entity) in entities.iter().enumerate() {
            if entity.mask & !KNOWN_MASK != 0 || entity.mask != masks[index] {
                return Err(Error::Mask);
            }
        }
        for string in strings {
            let start = string.offset as u64;
            let end = start
                .checked_add(string.len as u64)
                .ok_or(Error::Overflow)?;
            if end > string_bytes.len() as u64 {
                return Err(Error::Range);
            }
            let start = string.offset as usize;
            let end = end as usize;
            if std::str::from_utf8(&string_bytes[start..end]).is_err() {
                return Err(Error::Utf8);
            }
        }
        hierarchy(entities, header.first_root)?;
        Ok(Self {
            entities,
            locals,
            worlds,
            meshes,
            cameras,
            lights,
            strings,
            string_bytes,
        })
    }

    pub fn entities(&self) -> &'a [CookedEntity] {
        self.entities
    }

    pub fn locals(&self) -> &'a [CookedTransform] {
        self.locals
    }

    pub fn worlds(&self) -> &'a [Mat4] {
        self.worlds
    }

    pub fn meshes(&self) -> &'a [CookedMesh] {
        self.meshes
    }

    pub fn cameras(&self) -> &'a [CookedCamera] {
        self.cameras
    }

    pub fn lights(&self) -> &'a [CookedLight] {
        self.lights
    }

    pub fn name(&self, entity: u32) -> Option<&'a str> {
        let index = self.entities.get(entity as usize)?.name;
        self.string(index)
    }

    pub fn instantiate(&self) -> Scene {
        let mut scene = Scene::new();
        let mut spawned = Vec::with_capacity(self.entities.len());
        for (index, entity) in self.entities.iter().enumerate() {
            let spawned_entity = if entity.parent == NONE {
                scene.spawn()
            } else {
                scene
                    .spawn_child(spawned[entity.parent as usize])
                    .expect("parent is live")
            };
            let local = self.locals[index].to_transform();
            scene
                .set_local_position(spawned_entity, local.translation)
                .expect("fresh entity");
            scene
                .set_local_rotation(spawned_entity, local.rotation)
                .expect("fresh entity");
            scene
                .set_local_scale(spawned_entity, local.scale)
                .expect("fresh entity");
            if let Some(name) = self.string(entity.name) {
                scene
                    .insert(spawned_entity, Name::new(name))
                    .expect("fresh entity");
            }
            spawned.push(spawned_entity);
        }
        for mesh in self.meshes {
            scene
                .insert(
                    spawned[mesh.entity as usize],
                    MeshRenderer {
                        model: Model::Primitive(
                            Primitive::from_kind(mesh.kind).expect("validated primitive"),
                        ),
                        material: AssetId(mesh.material),
                    },
                )
                .expect("fresh entity");
        }
        for camera in self.cameras {
            scene
                .insert(
                    spawned[camera.entity as usize],
                    Camera {
                        fov_y: camera.fov_y,
                        near: camera.near,
                        far: camera.far,
                    },
                )
                .expect("fresh entity");
        }
        for light in self.lights {
            scene
                .insert(
                    spawned[light.entity as usize],
                    Light::Directional {
                        color: Vec3::new(light.color[0], light.color[1], light.color[2]),
                        intensity: light.intensity,
                    },
                )
                .expect("fresh entity");
        }
        scene.update();
        scene
    }

    fn string(&self, index: u32) -> Option<&'a str> {
        if index == NONE {
            return None;
        }
        let string = self.strings.get(index as usize)?;
        let start = string.offset as usize;
        let end = start + string.len as usize;
        std::str::from_utf8(&self.string_bytes[start..end]).ok()
    }
}

fn section<'a, T: Pod>(
    bytes: &'a [u8],
    offset: u32,
    count: u32,
    spans: &mut Vec<(u64, u64)>,
    len: u64,
) -> Result<&'a [T], Error> {
    let Some((start, end)) = span(offset, count, size_of::<T>(), align_of::<T>(), len)? else {
        return Ok(&[]);
    };
    let pointer = bytes.as_ptr() as usize + start as usize;
    if pointer % align_of::<T>() != 0 {
        return Err(Error::Misaligned);
    }
    spans.push((start, end));
    try_cast_slice(&bytes[start as usize..end as usize]).map_err(|_| Error::Misaligned)
}

fn raw_section<'a>(
    bytes: &'a [u8],
    offset: u32,
    count: u32,
    spans: &mut Vec<(u64, u64)>,
    len: u64,
) -> Result<&'a [u8], Error> {
    let Some((start, end)) = span(offset, count, 1, 1, len)? else {
        return Ok(&[]);
    };
    spans.push((start, end));
    Ok(&bytes[start as usize..end as usize])
}

fn span(
    offset: u32,
    count: u32,
    stride: usize,
    align: usize,
    len: u64,
) -> Result<Option<(u64, u64)>, Error> {
    if count == 0 {
        if offset != 0 {
            return Err(Error::Range);
        }
        return Ok(None);
    }
    if offset as usize % align != 0 {
        return Err(Error::Misaligned);
    }
    let bytes = (count as u64)
        .checked_mul(stride as u64)
        .ok_or(Error::Overflow)?;
    let start = offset as u64;
    let end = start.checked_add(bytes).ok_or(Error::Overflow)?;
    if start < HEADER_SIZE as u64 {
        return Err(Error::Overlap);
    }
    if end > len {
        return Err(Error::Truncated);
    }
    Ok(Some((start, end)))
}

fn disjoint(spans: &mut [(u64, u64)]) -> Result<(), Error> {
    spans.sort_by_key(|span| span.0);
    for pair in spans.windows(2) {
        if pair[1].0 < pair[0].1 {
            return Err(Error::Overlap);
        }
    }
    Ok(())
}

fn link_index(value: u32, count: u32) -> Result<(), Error> {
    if value == NONE || value < count {
        Ok(())
    } else {
        Err(Error::Range)
    }
}

fn column<T: ColumnRecord>(
    records: &[T],
    entities: u32,
    bit: u32,
    masks: &mut [u32],
) -> Result<(), Error> {
    let mut previous = None;
    for record in records {
        let entity = record.entity();
        if entity >= entities {
            return Err(Error::Range);
        }
        if previous.is_some_and(|index| entity <= index) {
            return Err(Error::Column);
        }
        previous = Some(entity);
        masks[entity as usize] |= bit;
    }
    Ok(())
}

trait ColumnRecord {
    fn entity(&self) -> u32;
}

impl ColumnRecord for CookedMesh {
    fn entity(&self) -> u32 {
        self.entity
    }
}

impl ColumnRecord for CookedCamera {
    fn entity(&self) -> u32 {
        self.entity
    }
}

impl ColumnRecord for CookedLight {
    fn entity(&self) -> u32 {
        self.entity
    }
}

fn hierarchy(entities: &[CookedEntity], first_root: u32) -> Result<(), Error> {
    let mut seen = vec![false; entities.len()];
    let mut stack = vec![(first_root, NONE)];
    let mut visited = 0usize;
    while let Some((mut cursor, expected_parent)) = stack.pop() {
        while cursor != NONE {
            if visited == entities.len() {
                return Err(Error::Hierarchy);
            }
            let index = cursor as usize;
            if index >= entities.len() || seen[index] {
                return Err(Error::Hierarchy);
            }
            seen[index] = true;
            visited += 1;
            let entity = &entities[index];
            if entity.parent != expected_parent {
                return Err(Error::Hierarchy);
            }
            if entity.first_child != NONE {
                stack.push((entity.first_child, cursor));
            }
            cursor = entity.next_sibling;
        }
    }
    if visited != entities.len() {
        Err(Error::Hierarchy)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use glam::Mat4;

    use super::*;
    use crate::{MeshRenderer, Model, Name, Primitive, Scene, Vec3};

    fn sample() -> (Scene, AssetId) {
        let mut scene = Scene::new();
        let root = scene.spawn();
        scene.insert(root, Name::new("Root")).unwrap();
        let child = scene.spawn_child(root).unwrap();
        scene.set_local_position(child, Vec3::X).unwrap();
        let material = AssetId([9; 16]);
        scene
            .insert(
                child,
                MeshRenderer {
                    model: Model::Primitive(Primitive::Cube),
                    material,
                },
            )
            .unwrap();
        scene.update();
        (scene, material)
    }

    fn flat_from(
        scene: &Scene,
    ) -> (
        Vec<CookedEntity>,
        Vec<CookedTransform>,
        Vec<Mat4>,
        Vec<CookedMesh>,
        Vec<CookedStr>,
        Vec<u8>,
        u32,
    ) {
        let mut order = Vec::new();
        fn walk(scene: &Scene, entity: crate::Entity, order: &mut Vec<crate::Entity>) {
            order.push(entity);
            for child in scene.children(entity).unwrap() {
                walk(scene, child, order);
            }
        }
        for root in scene.roots() {
            walk(scene, root, &mut order);
        }
        let mut index_of = std::collections::HashMap::new();
        for (index, entity) in order.iter().enumerate() {
            index_of.insert(*entity, index as u32);
        }
        let mut strings = Vec::new();
        let mut string_bytes = Vec::new();
        let mut entities = Vec::new();
        let mut locals = Vec::new();
        let mut worlds = Vec::new();
        let mut meshes = Vec::new();
        let mut child_lists = vec![Vec::new(); order.len()];
        let mut roots = Vec::new();
        for (index, entity) in order.iter().enumerate() {
            match scene.parent(*entity).unwrap() {
                Some(parent) => child_lists[index_of[&parent] as usize].push(index as u32),
                None => roots.push(index as u32),
            }
        }
        for (index, entity) in order.iter().enumerate() {
            let name = scene.get::<Name>(*entity).unwrap().map(|name| {
                let offset = string_bytes.len() as u32;
                let bytes = name.as_str().as_bytes();
                string_bytes.extend_from_slice(bytes);
                let string = strings.len() as u32;
                strings.push(CookedStr {
                    offset,
                    len: bytes.len() as u32,
                });
                string
            });
            let mut mask = 0;
            if name.is_some() {
                mask |= ComponentId::Name as u32;
            }
            if scene.get::<MeshRenderer>(*entity).unwrap().is_some() {
                mask |= ComponentId::Mesh as u32;
            }
            let first_child = child_lists[index].first().copied().unwrap_or(NONE);
            entities.push(CookedEntity {
                parent: scene
                    .parent(*entity)
                    .unwrap()
                    .map(|parent| index_of[&parent])
                    .unwrap_or(NONE),
                first_child,
                next_sibling: NONE,
                name: name.unwrap_or(NONE),
                mask,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            });
            locals.push(CookedTransform::from_transform(
                scene.local(*entity).unwrap(),
            ));
            worlds.push(scene.world(*entity).unwrap());
            if let Some(mesh) = scene.get::<MeshRenderer>(*entity).unwrap() {
                let Model::Primitive(primitive) = mesh.model;
                meshes.push(CookedMesh {
                    entity: index as u32,
                    kind: primitive.kind(),
                    _pad: [0; 8],
                    material: mesh.material.0,
                });
            }
        }
        let first_root = roots.first().copied().unwrap_or(NONE);
        link_siblings(&mut entities, &roots);
        for children in &child_lists {
            link_siblings(&mut entities, children);
        }
        (
            entities,
            locals,
            worlds,
            meshes,
            strings,
            string_bytes,
            first_root,
        )
    }

    #[test]
    fn pod_layout_matches_the_blob() {
        assert_eq!(size_of::<Header>(), 128);
        assert_eq!(size_of::<CookedTransform>(), 48);
        assert_eq!(std::mem::offset_of!(CookedMesh, material), 16);
    }

    #[test]
    fn validate_reads_worlds_and_instantiate_keeps_material() {
        let (scene, material) = sample();
        let (entities, locals, worlds, meshes, strings, string_bytes, first_root) =
            flat_from(&scene);
        let bytes = write(&Flat {
            first_root,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &strings,
            string_bytes: &string_bytes,
        })
        .unwrap();
        assert_eq!(bytes.as_slice().as_ptr() as usize % ALIGN, 0);
        let cooked = CookedScene::validate(bytes.as_slice()).unwrap();
        assert_eq!(cooked.worlds(), worlds.as_slice());
        assert_eq!(cooked.name(0), Some("Root"));
        let loaded = cooked.instantiate();
        let child = loaded
            .children(loaded.roots().next().unwrap())
            .unwrap()
            .next()
            .unwrap();
        assert_eq!(
            loaded.get::<MeshRenderer>(child).unwrap().unwrap().material,
            material
        );
        assert_eq!(
            loaded.get::<MeshRenderer>(child).unwrap().unwrap().model,
            Model::Primitive(Primitive::Cube)
        );
        assert_eq!(
            loaded.world(child).unwrap(),
            scene
                .world(
                    scene
                        .children(scene.roots().next().unwrap())
                        .unwrap()
                        .next()
                        .unwrap()
                )
                .unwrap()
        );
    }

    #[test]
    fn sphere_and_wedge_roundtrip() {
        let mut scene = Scene::new();
        let sphere = scene.spawn();
        let wedge = scene.spawn_child(sphere).unwrap();
        let sphere_material = AssetId([3; 16]);
        let wedge_material = AssetId([4; 16]);
        scene
            .insert(
                sphere,
                MeshRenderer {
                    model: Model::Primitive(Primitive::Sphere),
                    material: sphere_material,
                },
            )
            .unwrap();
        scene
            .insert(
                wedge,
                MeshRenderer {
                    model: Model::Primitive(Primitive::Wedge),
                    material: wedge_material,
                },
            )
            .unwrap();
        scene.update();
        let (entities, locals, worlds, meshes, strings, string_bytes, first_root) =
            flat_from(&scene);
        assert_eq!(meshes[0].kind, MESH_SPHERE);
        assert_eq!(meshes[1].kind, MESH_WEDGE);
        let bytes = write(&Flat {
            first_root,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &strings,
            string_bytes: &string_bytes,
        })
        .unwrap();
        let loaded = CookedScene::validate(bytes.as_slice())
            .unwrap()
            .instantiate();
        let root = loaded.roots().next().unwrap();
        assert_eq!(
            loaded.get::<MeshRenderer>(root).unwrap().unwrap().model,
            Model::Primitive(Primitive::Sphere)
        );
        let child = loaded.children(root).unwrap().next().unwrap();
        assert_eq!(
            loaded.get::<MeshRenderer>(child).unwrap().unwrap().model,
            Model::Primitive(Primitive::Wedge)
        );
    }

    #[test]
    fn shifted_slice_is_misaligned() {
        let (scene, _) = sample();
        let (entities, locals, worlds, meshes, strings, string_bytes, first_root) =
            flat_from(&scene);
        let bytes = write(&Flat {
            first_root,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &strings,
            string_bytes: &string_bytes,
        })
        .unwrap();
        let mut prefixed = vec![0u8];
        prefixed.extend_from_slice(bytes.as_slice());
        let aligned = AlignedBytes::from_slice(&prefixed);
        let shifted = &aligned.as_slice()[1..];
        assert_eq!(shifted.as_ptr() as usize % ALIGN, 1);
        assert!(matches!(
            CookedScene::validate(shifted),
            Err(Error::Misaligned)
        ));
    }

    #[test]
    fn overlapping_sections_are_rejected() {
        let (scene, _) = sample();
        let (entities, locals, worlds, meshes, strings, string_bytes, first_root) =
            flat_from(&scene);
        let mut bytes = write(&Flat {
            first_root,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &strings,
            string_bytes: &string_bytes,
        })
        .unwrap();
        let entities_off = u32::from_le_bytes(bytes.as_slice()[36..40].try_into().unwrap());
        bytes.as_mut_slice()[48..52].copy_from_slice(&entities_off.to_le_bytes());
        bytes.as_mut_slice()[12..16].copy_from_slice(&1u32.to_le_bytes());
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Overlap)
        ));
    }

    #[test]
    fn sibling_cycle_and_unreachable_entity_are_rejected() {
        let (entities, locals, worlds) = pair();
        let mut cycled = entities.clone();
        cycled[1].next_sibling = 1;
        let bytes = write(&empty_flat(&cycled, &locals, &worlds, 0)).unwrap();
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Hierarchy)
        ));
        let mut orphan = entities.clone();
        orphan[0].first_child = NONE;
        orphan[1].parent = NONE;
        let bytes = write(&empty_flat(&orphan, &locals, &worlds, 0)).unwrap();
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Hierarchy)
        ));
    }

    #[test]
    fn unsorted_column_unknown_kind_and_bad_utf8_are_rejected() {
        let (mut entities, locals, worlds) = pair();
        entities[0].mask |= ComponentId::Mesh as u32;
        entities[1].mask |= ComponentId::Mesh as u32;
        let meshes = [
            CookedMesh {
                entity: 1,
                kind: MESH_CUBE,
                _pad: [0; 8],
                material: [1; 16],
            },
            CookedMesh {
                entity: 0,
                kind: MESH_CUBE,
                _pad: [0; 8],
                material: [2; 16],
            },
        ];
        let bytes = write(&Flat {
            first_root: 0,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &[],
            string_bytes: &[],
        })
        .unwrap();
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Column)
        ));
        let meshes = [CookedMesh {
            entity: 1,
            kind: 7,
            _pad: [0; 8],
            material: [1; 16],
        }];
        entities[0].mask = 0;
        let bytes = write(&Flat {
            first_root: 0,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &meshes,
            cameras: &[],
            lights: &[],
            strings: &[],
            string_bytes: &[],
        })
        .unwrap();
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Kind)
        ));
        entities[0].mask = ComponentId::Name as u32;
        entities[0].name = 0;
        entities[1].mask = 0;
        let strings = [CookedStr { offset: 0, len: 1 }];
        let bytes = write(&Flat {
            first_root: 0,
            entities: &entities,
            locals: &locals,
            worlds: &worlds,
            meshes: &[],
            cameras: &[],
            lights: &[],
            strings: &strings,
            string_bytes: &[0xff],
        })
        .unwrap();
        assert!(matches!(
            CookedScene::validate(bytes.as_slice()),
            Err(Error::Utf8)
        ));
    }

    fn link_siblings(entities: &mut [CookedEntity], chain: &[u32]) {
        for (index, entity) in chain.iter().enumerate() {
            entities[*entity as usize].next_sibling = chain.get(index + 1).copied().unwrap_or(NONE);
        }
    }

    fn pair() -> (Vec<CookedEntity>, Vec<CookedTransform>, Vec<Mat4>) {
        let entities = vec![
            CookedEntity {
                parent: NONE,
                first_child: 1,
                next_sibling: NONE,
                name: NONE,
                mask: 0,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
            CookedEntity {
                parent: 0,
                first_child: NONE,
                next_sibling: NONE,
                name: NONE,
                mask: 0,
                _pad0: 0,
                _pad1: 0,
                _pad2: 0,
            },
        ];
        let local = CookedTransform::from_transform(Transform::default());
        let locals = vec![local, local];
        let worlds = vec![Mat4::IDENTITY, Mat4::IDENTITY];
        (entities, locals, worlds)
    }

    fn empty_flat<'a>(
        entities: &'a [CookedEntity],
        locals: &'a [CookedTransform],
        worlds: &'a [Mat4],
        first_root: u32,
    ) -> Flat<'a> {
        Flat {
            first_root,
            entities,
            locals,
            worlds,
            meshes: &[],
            cameras: &[],
            lights: &[],
            strings: &[],
            string_bytes: &[],
        }
    }
}
