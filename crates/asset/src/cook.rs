use std::collections::HashMap;

use scene::cooked::{
    self, CookedCamera, CookedEntity, CookedLight, CookedMesh, CookedStr, CookedTransform, Flat,
    NONE,
};
use scene::{ComponentId, Mat4};

use crate::document::Document;
use crate::resolve::{self, ResolvedNode};
use crate::{Assets, Error};

pub fn cook(
    document: &Document,
    assets: &Assets,
    path: Option<&str>,
) -> Result<cooked::AlignedBytes, Error> {
    let resolved = resolve::resolve(document, assets, path)?;
    emit(&resolved)
}

fn emit(nodes: &[ResolvedNode]) -> Result<cooked::AlignedBytes, Error> {
    let mut child_lists = vec![Vec::new(); nodes.len()];
    let mut roots = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        match node.parent {
            Some(parent) => child_lists[parent as usize].push(index as u32),
            None => roots.push(index as u32),
        }
    }
    let mut strings = Strings::default();
    let mut entities = Vec::with_capacity(nodes.len());
    let mut locals = Vec::with_capacity(nodes.len());
    let mut worlds = vec![Mat4::IDENTITY; nodes.len()];
    let mut meshes = Vec::new();
    let mut cameras = Vec::new();
    let mut lights = Vec::new();
    for (index, node) in nodes.iter().enumerate() {
        let local = Mat4::from_scale_rotation_translation(
            node.transform.scale,
            node.transform.rotation,
            node.transform.translation,
        );
        worlds[index] = match node.parent {
            Some(parent) => worlds[parent as usize] * local,
            None => local,
        };
        let mut mask = 0;
        let name = if let Some(name) = &node.name {
            mask |= ComponentId::Name as u32;
            strings.intern(name)
        } else {
            NONE
        };
        if node.mesh.is_some() {
            mask |= ComponentId::Mesh as u32;
        }
        if node.camera.is_some() {
            mask |= ComponentId::Camera as u32;
        }
        if node.light.is_some() {
            mask |= ComponentId::Light as u32;
        }
        entities.push(CookedEntity {
            parent: node.parent.unwrap_or(NONE),
            first_child: child_lists[index].first().copied().unwrap_or(NONE),
            next_sibling: NONE,
            name,
            mask,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        });
        locals.push(CookedTransform::from_transform(node.transform));
        if let Some(material) = node.mesh {
            meshes.push(CookedMesh {
                entity: index as u32,
                kind: cooked::MESH_CUBE,
                _pad: [0; 8],
                material: material.0,
            });
        }
        if let Some(camera) = node.camera {
            cameras.push(CookedCamera {
                entity: index as u32,
                _pad0: 0,
                fov_y: camera.fov_y,
                near: camera.near,
                far: camera.far,
                _pad1: [0.0; 3],
            });
        }
        if let Some(light) = node.light {
            let scene::Light::Directional { color, intensity } = light;
            lights.push(CookedLight {
                entity: index as u32,
                kind: cooked::LIGHT_DIRECTIONAL,
                color: color.to_array(),
                intensity,
                _pad: [0.0; 2],
            });
        }
    }
    link(&mut entities, &roots);
    for children in &child_lists {
        link(&mut entities, children);
    }
    meshes.sort_by_key(|mesh| mesh.entity);
    cameras.sort_by_key(|camera| camera.entity);
    lights.sort_by_key(|light| light.entity);
    let bytes = cooked::write(&Flat {
        first_root: roots.first().copied().unwrap_or(NONE),
        entities: &entities,
        locals: &locals,
        worlds: &worlds,
        meshes: &meshes,
        cameras: &cameras,
        lights: &lights,
        strings: &strings.entries,
        string_bytes: &strings.bytes,
    })?;
    Ok(bytes)
}

fn link(entities: &mut [CookedEntity], chain: &[u32]) {
    for (index, entity) in chain.iter().enumerate() {
        entities[*entity as usize].next_sibling = chain.get(index + 1).copied().unwrap_or(NONE);
    }
}

#[derive(Default)]
struct Strings {
    entries: Vec<CookedStr>,
    bytes: Vec<u8>,
    index: HashMap<String, u32>,
}

impl Strings {
    fn intern(&mut self, value: &str) -> u32 {
        if let Some(index) = self.index.get(value) {
            return *index;
        }
        let offset = self.bytes.len() as u32;
        self.bytes.extend_from_slice(value.as_bytes());
        let index = self.entries.len() as u32;
        self.entries.push(CookedStr {
            offset,
            len: value.len() as u32,
        });
        self.index.insert(value.to_string(), index);
        index
    }
}
