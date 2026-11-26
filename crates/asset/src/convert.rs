use scene::{AssetId, Camera, Light, MeshRenderer, Model, Name, Scene};

use crate::document::{CameraValue, Document, LightValue, MeshValue, Node, PrimitiveValue};
use crate::resolve::{self, from_transform};
use crate::{Assets, Error};

pub fn document_to_scene(document: &Document, assets: &Assets) -> Result<Scene, Error> {
    let resolved = resolve::resolve(document, assets, None)?;
    let mut scene = Scene::new();
    let mut spawned = Vec::with_capacity(resolved.len());
    for node in &resolved {
        let entity = match node.parent {
            Some(parent) => scene
                .spawn_child(spawned[parent as usize])
                .expect("parent is live"),
            None => scene.spawn(),
        };
        scene
            .set_local_position(entity, node.transform.translation)
            .expect("fresh entity");
        scene
            .set_local_rotation(entity, node.transform.rotation)
            .expect("fresh entity");
        scene
            .set_local_scale(entity, node.transform.scale)
            .expect("fresh entity");
        if let Some(name) = &node.name {
            scene
                .insert(entity, Name::new(name.clone()))
                .expect("fresh entity");
        }
        if let Some(mesh) = node.mesh {
            scene
                .insert(
                    entity,
                    MeshRenderer {
                        model: Model::Primitive(mesh.primitive),
                        material: mesh.material,
                    },
                )
                .expect("fresh entity");
        }
        if let Some(camera) = node.camera {
            scene.insert(entity, camera).expect("fresh entity");
        }
        if let Some(light) = node.light {
            scene.insert(entity, light).expect("fresh entity");
        }
        spawned.push(entity);
    }
    scene.update();
    Ok(scene)
}

pub fn scene_to_document(scene: &Scene, guid: &str, assets: &Assets) -> Result<Document, Error> {
    let guid = AssetId::parse(guid).ok_or_else(|| Error::Guid(guid.to_string()))?;
    let mut order = Vec::new();
    fn walk(scene: &Scene, entity: scene::Entity, order: &mut Vec<scene::Entity>) {
        order.push(entity);
        for child in scene.children(entity).expect("live entity") {
            walk(scene, child, order);
        }
    }
    for root in scene.roots() {
        walk(scene, root, &mut order);
    }
    let mut ids = std::collections::HashMap::new();
    for (index, entity) in order.iter().enumerate() {
        ids.insert(*entity, (index as u32) + 1);
    }
    let mut nodes = Vec::with_capacity(order.len());
    for entity in order {
        let parent = scene
            .parent(entity)
            .expect("live entity")
            .map(|parent| ids[&parent]);
        let mesh = scene
            .get::<MeshRenderer>(entity)
            .expect("live entity")
            .map(|mesh| {
                let Model::Primitive(primitive) = mesh.model;
                MeshValue {
                    primitive: PrimitiveValue::from_primitive(primitive),
                    material: assets
                        .path(mesh.material)
                        .unwrap_or_else(|| mesh.material.canonical()),
                }
            });
        let camera = scene
            .get::<Camera>(entity)
            .expect("live entity")
            .copied()
            .map(|camera| CameraValue {
                fov_y: camera.fov_y,
                near: camera.near,
                far: camera.far,
            });
        let light = scene
            .get::<Light>(entity)
            .expect("live entity")
            .copied()
            .map(|light| {
                let Light::Directional { color, intensity } = light;
                LightValue {
                    color: color.to_array(),
                    intensity,
                }
            });
        nodes.push(Node {
            id: ids[&entity],
            parent,
            name: scene
                .get::<Name>(entity)
                .expect("live entity")
                .map(|name| name.as_str().to_string()),
            transform: from_transform(scene.local(entity).expect("live entity")),
            mesh,
            camera,
            light,
            prefab: None,
        });
    }
    Ok(Document {
        guid: guid.canonical(),
        nodes,
    })
}
