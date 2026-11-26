use std::collections::HashMap;

use scene::{AssetId, Camera, Light, Quat, Transform, Vec3};

use crate::document::{Document, Node, Override, TransformValue};
use crate::{Assets, Error};

#[derive(Clone, Debug)]
pub struct ResolvedNode {
    pub parent: Option<u32>,
    pub name: Option<String>,
    pub transform: Transform,
    pub mesh: Option<AssetId>,
    pub camera: Option<Camera>,
    pub light: Option<Light>,
}

pub fn resolve(
    document: &Document,
    assets: &Assets,
    path: Option<&str>,
) -> Result<Vec<ResolvedNode>, Error> {
    let mut stack = Vec::new();
    if let Some(path) = path {
        stack.push(path.to_string());
    }
    let mut expanded = Vec::new();
    expand(document, assets, None, &mut stack, &mut expanded)?;
    let mut resolved = Vec::with_capacity(expanded.len());
    for node in expanded {
        resolved.push(ResolvedNode {
            parent: node.parent,
            name: node.name,
            transform: to_transform(node.id, &node.transform)?,
            mesh: node
                .mesh
                .map(|mesh| assets.material(&mesh.material))
                .transpose()?,
            camera: node.camera.map(|camera| Camera {
                fov_y: camera.fov_y,
                near: camera.near,
                far: camera.far,
            }),
            light: node.light.map(|light| Light::Directional {
                color: Vec3::from_array(light.color),
                intensity: light.intensity,
            }),
        });
    }
    Ok(resolved)
}

struct Expanded {
    id: u32,
    parent: Option<u32>,
    name: Option<String>,
    transform: TransformValue,
    mesh: Option<crate::document::MeshValue>,
    camera: Option<crate::document::CameraValue>,
    light: Option<crate::document::LightValue>,
}

fn expand(
    document: &Document,
    assets: &Assets,
    attach: Option<u32>,
    stack: &mut Vec<String>,
    output: &mut Vec<Expanded>,
) -> Result<(), Error> {
    check_links(&document.nodes)?;
    for index in root_indexes(&document.nodes) {
        place(&document.nodes, index, attach, assets, stack, output)?;
    }
    Ok(())
}

fn place(
    nodes: &[Node],
    index: usize,
    parent: Option<u32>,
    assets: &Assets,
    stack: &mut Vec<String>,
    output: &mut Vec<Expanded>,
) -> Result<(), Error> {
    let node = &nodes[index];
    if let Some(prefab) = &node.prefab {
        if nodes.iter().any(|other| other.parent == Some(node.id)) {
            return Err(Error::PrefabChildren(node.id));
        }
        if stack.iter().any(|path| path == &prefab.source) {
            return Err(Error::Cycle(prefab.source.clone()));
        }
        let source = assets
            .scene(&prefab.source)
            .ok_or_else(|| Error::UnknownAsset(prefab.source.clone()))?;
        let mut inner = source.nodes.clone();
        check_links(&inner)?;
        let roots = root_indexes(&inner);
        if roots.len() != 1 {
            return Err(Error::PrefabRoot(prefab.source.clone()));
        }
        let root = &mut inner[roots[0]];
        root.transform = node.transform.clone();
        if node.name.is_some() {
            root.name.clone_from(&node.name);
        }
        if node.mesh.is_some() {
            root.mesh.clone_from(&node.mesh);
        }
        if node.camera.is_some() {
            root.camera.clone_from(&node.camera);
        }
        if node.light.is_some() {
            root.light.clone_from(&node.light);
        }
        for replacement in &prefab.overrides {
            apply_override(&mut inner, replacement)?;
        }
        stack.push(prefab.source.clone());
        let nested = Document {
            guid: source.guid.clone(),
            nodes: inner,
        };
        expand(&nested, assets, parent, stack, output)?;
        stack.pop();
        return Ok(());
    }
    let index_in_output = output.len() as u32;
    output.push(Expanded {
        id: node.id,
        parent,
        name: node.name.clone(),
        transform: node.transform.clone(),
        mesh: node.mesh.clone(),
        camera: node.camera.clone(),
        light: node.light.clone(),
    });
    for child in nodes
        .iter()
        .enumerate()
        .filter(|(_, child)| child.parent == Some(node.id))
        .map(|(index, _)| index)
    {
        place(nodes, child, Some(index_in_output), assets, stack, output)?;
    }
    Ok(())
}

fn apply_override(nodes: &mut [Node], replacement: &Override) -> Result<(), Error> {
    let node = nodes
        .iter_mut()
        .find(|node| node.id == replacement.id)
        .ok_or(Error::MissingId(replacement.id))?;
    if let Some(transform) = &replacement.transform {
        node.transform = transform.clone();
    }
    if let Some(name) = &replacement.name {
        node.name = Some(name.clone());
    }
    if let Some(mesh) = &replacement.mesh {
        node.mesh = Some(mesh.clone());
    }
    if let Some(camera) = &replacement.camera {
        node.camera = Some(camera.clone());
    }
    if let Some(light) = &replacement.light {
        node.light = Some(light.clone());
    }
    Ok(())
}

fn check_links(nodes: &[Node]) -> Result<(), Error> {
    let mut ids = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        if ids.insert(node.id, index).is_some() {
            return Err(Error::DuplicateId(node.id));
        }
    }
    for node in nodes {
        if let Some(parent) = node.parent
            && !ids.contains_key(&parent)
        {
            return Err(Error::MissingParent(parent));
        }
    }
    let mut state = vec![0u8; nodes.len()];
    for start in 0..nodes.len() {
        if state[start] != 0 {
            continue;
        }
        let mut path = Vec::new();
        let mut cursor = start;
        loop {
            if state[cursor] == 2 {
                break;
            }
            if state[cursor] == 1 {
                return Err(Error::NodeCycle(nodes[cursor].id));
            }
            state[cursor] = 1;
            path.push(cursor);
            match nodes[cursor].parent {
                None => break,
                Some(parent) => cursor = ids[&parent],
            }
        }
        for index in path {
            state[index] = 2;
        }
    }
    Ok(())
}

fn root_indexes(nodes: &[Node]) -> Vec<usize> {
    nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| node.parent.is_none())
        .map(|(index, _)| index)
        .collect()
}

pub fn to_transform(id: u32, value: &TransformValue) -> Result<Transform, Error> {
    let rotation = Quat::from_xyzw(
        value.rotation[0],
        value.rotation[1],
        value.rotation[2],
        value.rotation[3],
    );
    if !rotation.is_normalized() {
        return Err(Error::Rotation(id));
    }
    Ok(Transform {
        translation: Vec3::from_array(value.translation),
        rotation,
        scale: Vec3::from_array(value.scale),
    })
}

pub fn from_transform(value: Transform) -> TransformValue {
    TransformValue {
        translation: value.translation.to_array(),
        rotation: value.rotation.to_array(),
        scale: value.scale.to_array(),
    }
}
