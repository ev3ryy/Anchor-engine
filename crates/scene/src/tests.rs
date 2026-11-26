use super::{
    AssetId, Camera, Column, ComponentId, ComponentMask, Entity, Error, Extract, Light, Mat4,
    MaterialRef, MeshRenderer, Model, Name, Primitive, Quat, Scene, Vec3,
};

fn any_material(_: AssetId) -> Option<MaterialRef> {
    Some(MaterialRef {
        index: 1,
        generation: 1,
    })
}

fn asset(byte: u8) -> AssetId {
    let mut bytes = [0; 16];
    bytes[0] = byte;
    AssetId(bytes)
}

fn translation(scene: &Scene, entity: Entity) -> Vec3 {
    scene.world(entity).unwrap().w_axis.truncate()
}

fn close(found: Vec3, expected: Vec3) {
    let delta = found - expected;
    assert!(delta.length() < 1.0e-4, "translation {found} != {expected}");
}

#[test]
fn generation_bumps_on_despawn_and_reuse() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    let destroyed = scene.despawn(entity).unwrap();
    assert_eq!(destroyed, vec![entity]);
    scene.update();
    assert!(matches!(scene.local(entity), Err(Error::StaleEntity)));
    assert!(matches!(scene.world(entity), Err(Error::StaleEntity)));

    let reused = scene.spawn();
    assert_eq!(reused.index(), entity.index());
    assert_ne!(reused.generation(), entity.generation());
    scene.update();
    assert!(scene.local(reused).is_ok());
    assert!(matches!(scene.local(entity), Err(Error::StaleEntity)));
    assert!(matches!(
        scene.set_local_position(entity, Vec3::X),
        Err(Error::StaleEntity)
    ));
}

#[test]
fn world_transform_reaches_three_levels() {
    let mut scene = Scene::new();
    let root = scene.spawn();
    let other = scene.spawn();
    let mid = scene.spawn_child(root).unwrap();
    let leaf = scene.spawn_child(mid).unwrap();
    scene
        .set_local_position(root, Vec3::new(0.0, 2.0, 0.0))
        .unwrap();
    scene
        .set_local_rotation(mid, Quat::from_rotation_y(std::f32::consts::FRAC_PI_2))
        .unwrap();
    scene.set_local_position(leaf, Vec3::X).unwrap();
    scene
        .set_local_position(other, Vec3::new(5.0, 0.0, 0.0))
        .unwrap();
    scene.update();

    close(translation(&scene, root), Vec3::new(0.0, 2.0, 0.0));
    close(translation(&scene, mid), Vec3::new(0.0, 2.0, 0.0));
    close(translation(&scene, leaf), Vec3::new(0.0, 2.0, -1.0));
    close(translation(&scene, other), Vec3::new(5.0, 0.0, 0.0));
}

#[test]
fn reparent_updates_world() {
    let mut scene = Scene::new();
    let left = scene.spawn();
    let right = scene.spawn();
    let child = scene.spawn_child(left).unwrap();
    scene
        .set_local_position(right, Vec3::new(10.0, 0.0, 0.0))
        .unwrap();
    scene.set_local_position(child, Vec3::X).unwrap();
    scene.update();
    close(translation(&scene, child), Vec3::X);

    scene.set_parent(child, Some(right)).unwrap();
    scene.update();
    close(translation(&scene, child), Vec3::new(11.0, 0.0, 0.0));
    assert_eq!(scene.parent(child).unwrap(), Some(right));
    assert_eq!(scene.children(left).unwrap().count(), 0);
    assert_eq!(
        scene.children(right).unwrap().collect::<Vec<_>>(),
        vec![child]
    );
}

#[test]
fn parent_cycle_is_rejected() {
    let mut scene = Scene::new();
    let root = scene.spawn();
    let child = scene.spawn_child(root).unwrap();
    let grand = scene.spawn_child(child).unwrap();
    assert!(matches!(
        scene.set_parent(root, Some(root)),
        Err(Error::Cycle)
    ));
    assert!(matches!(
        scene.set_parent(root, Some(child)),
        Err(Error::Cycle)
    ));
    assert!(matches!(
        scene.set_parent(root, Some(grand)),
        Err(Error::Cycle)
    ));
    assert!(matches!(
        scene.set_parent(child, Some(grand)),
        Err(Error::Cycle)
    ));
    assert_eq!(scene.parent(root).unwrap(), None);
    assert_eq!(scene.parent(child).unwrap(), Some(root));
    assert_eq!(scene.parent(grand).unwrap(), Some(child));
}

#[test]
fn despawn_root_removes_subtree_and_keeps_sibling() {
    let mut scene = Scene::new();
    let keep = scene.spawn();
    let nephew = scene.spawn_child(keep).unwrap();
    let root = scene.spawn();
    let child = scene.spawn_child(root).unwrap();
    let grand = scene.spawn_child(child).unwrap();
    let destroyed = scene.despawn(root).unwrap();
    assert_eq!(destroyed.len(), 3);
    assert!(destroyed.contains(&root));
    assert!(destroyed.contains(&child));
    assert!(destroyed.contains(&grand));
    scene.update();
    assert_eq!(scene.len(), 2);
    assert_eq!(scene.roots().collect::<Vec<_>>(), vec![keep]);
    assert_eq!(
        scene.children(keep).unwrap().collect::<Vec<_>>(),
        vec![nephew]
    );
    assert!(scene.local(root).is_err());
    assert!(scene.local(child).is_err());
    assert!(scene.local(grand).is_err());
}

#[test]
fn sibling_order_follows_append() {
    let mut scene = Scene::new();
    let mut roots = Vec::new();
    for _ in 0..8 {
        roots.push(scene.spawn());
    }
    assert_eq!(scene.roots().collect::<Vec<_>>(), roots);

    let parent = scene.spawn();
    let mut children = Vec::new();
    for _ in 0..64 {
        children.push(scene.spawn_child(parent).unwrap());
    }
    assert_eq!(
        scene.children(parent).unwrap().collect::<Vec<_>>(),
        children
    );

    scene.despawn(children[0]).unwrap();
    scene.despawn(children[31]).unwrap();
    scene.despawn(children[63]).unwrap();
    let remaining: Vec<_> = children
        .into_iter()
        .enumerate()
        .filter(|(index, _)| *index != 0 && *index != 31 && *index != 63)
        .map(|(_, entity)| entity)
        .collect();
    assert_eq!(
        scene.children(parent).unwrap().collect::<Vec<_>>(),
        remaining
    );

    scene.despawn(roots[0]).unwrap();
    scene.despawn(roots[7]).unwrap();
    assert_eq!(scene.roots().next(), Some(roots[1]));
    assert_eq!(scene.roots().last(), Some(parent));
}

#[test]
fn column_misses_reused_slot() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    let mut column = Column::new();
    assert_eq!(column.insert(entity, 7u32), None);
    assert_eq!(column.get(entity), Some(&7));
    assert_eq!(column.iter().collect::<Vec<_>>(), vec![(entity, &7)]);

    scene.despawn(entity).unwrap();
    let reused = scene.spawn();
    assert_eq!(reused.index(), entity.index());
    assert!(column.get(reused).is_none());
    assert_eq!(column.get(entity), Some(&7));

    assert_eq!(column.insert(reused, 9), Some(7));
    assert_eq!(column.get(reused), Some(&9));
    assert!(column.get(entity).is_none());
    assert_eq!(column.remove(reused), Some(9));
    assert!(column.get(reused).is_none());
}

#[test]
fn local_is_readable_while_dirty() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.insert(entity, Name::new("Root")).unwrap();
    scene.update();
    scene.set_local_position(entity, Vec3::X).unwrap();
    assert_eq!(scene.local(entity).unwrap().translation, Vec3::X);
    assert!(
        scene
            .get::<Name>(entity)
            .unwrap()
            .is_some_and(|name| name == "Root")
    );
    scene.update();
    close(translation(&scene, entity), Vec3::X);
}

#[test]
#[cfg(debug_assertions)]
#[should_panic]
fn world_without_update_panics() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.update();
    scene.set_local_position(entity, Vec3::X).unwrap();
    let _ = scene.world(entity);
}

fn sample_components() -> (MeshRenderer, Camera, Light) {
    (
        MeshRenderer {
            model: Model::Primitive(Primitive::Cube),
            material: asset(3),
        },
        Camera {
            fov_y: 1.0,
            near: 0.1,
            far: 50.0,
        },
        Light::Directional {
            color: Vec3::ONE,
            intensity: 2.0,
        },
    )
}

#[test]
fn mesh_camera_and_light_roundtrip() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    let (mesh, camera, light) = sample_components();
    scene.insert(entity, mesh).unwrap();
    scene.insert(entity, camera).unwrap();
    scene.insert(entity, light).unwrap();
    assert_eq!(scene.get::<MeshRenderer>(entity).unwrap(), Some(&mesh));
    assert_eq!(scene.get::<Camera>(entity).unwrap(), Some(&camera));
    assert_eq!(scene.get::<Light>(entity).unwrap(), Some(&light));
    assert_eq!(
        scene.iter::<MeshRenderer>().collect::<Vec<_>>(),
        vec![(entity, &mesh)]
    );
    assert_eq!(scene.remove::<MeshRenderer>(entity).unwrap(), Some(mesh));
    assert_eq!(scene.remove::<Camera>(entity).unwrap(), Some(camera));
    assert_eq!(scene.remove::<Light>(entity).unwrap(), Some(light));
    assert_eq!(scene.get::<MeshRenderer>(entity).unwrap(), None);
    assert_eq!(scene.get::<Camera>(entity).unwrap(), None);
    assert_eq!(scene.get::<Light>(entity).unwrap(), None);
}

#[test]
fn despawn_clears_every_column() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    let (mesh, camera, light) = sample_components();
    scene.insert(entity, Name::new("Root")).unwrap();
    scene.insert(entity, mesh).unwrap();
    scene.insert(entity, camera).unwrap();
    scene.insert(entity, light).unwrap();
    scene.despawn(entity).unwrap();
    let reused = scene.spawn();
    assert_eq!(reused.index(), entity.index());
    assert_eq!(scene.get::<Name>(reused).unwrap(), None);
    assert_eq!(scene.get::<MeshRenderer>(reused).unwrap(), None);
    assert_eq!(scene.get::<Camera>(reused).unwrap(), None);
    assert_eq!(scene.get::<Light>(reused).unwrap(), None);
    assert!(scene.iter::<Name>().next().is_none());
    assert!(scene.iter::<MeshRenderer>().next().is_none());
    assert!(scene.iter::<Camera>().next().is_none());
    assert!(scene.iter::<Light>().next().is_none());
}

#[test]
fn extract_reads_child_world_and_reuses_capacity() {
    let mut scene = Scene::new();
    let root = scene.spawn();
    let child = scene.spawn_child(root).unwrap();
    scene.set_local_position(child, Vec3::X).unwrap();
    let mesh = MeshRenderer {
        model: Model::Primitive(Primitive::Cube),
        material: asset(1),
    };
    scene.insert(child, mesh).unwrap();
    scene
        .insert(
            child,
            Camera {
                fov_y: 0.8,
                near: 0.2,
                far: 20.0,
            },
        )
        .unwrap();
    scene
        .insert(
            child,
            Light::Directional {
                color: Vec3::new(0.2, 0.4, 0.6),
                intensity: 3.0,
            },
        )
        .unwrap();
    scene.update();
    let mut extract = Extract::default();
    scene.extract_into(&mut extract, any_material);
    assert_eq!(extract.draws.len(), 1);
    assert_eq!(extract.draws[0].entity, child);
    assert_eq!(extract.draws[0].model, Model::Primitive(Primitive::Cube));
    assert_eq!(extract.draws[0].world, scene.world(child).unwrap());
    assert_eq!(extract.cameras.len(), 1);
    assert_eq!(extract.cameras[0].fov_y, 0.8);
    close(extract.cameras[0].position, Vec3::X);
    assert_eq!(extract.lights.len(), 1);
    assert_eq!(extract.lights[0].intensity, 3.0);
    let draw_capacity = extract.draws.capacity();
    let camera_capacity = extract.cameras.capacity();
    let light_capacity = extract.lights.capacity();
    scene.remove::<MeshRenderer>(child).unwrap();
    scene.remove::<Camera>(child).unwrap();
    scene.remove::<Light>(child).unwrap();
    scene.extract_into(&mut extract, any_material);
    assert!(extract.draws.is_empty());
    assert!(extract.cameras.is_empty());
    assert!(extract.lights.is_empty());
    assert!(extract.draws.capacity() >= draw_capacity);
    assert!(extract.cameras.capacity() >= camera_capacity);
    assert!(extract.lights.capacity() >= light_capacity);
}

#[test]
fn extract_drops_degenerate_basis_and_keeps_mirrored_one() {
    let mut scene = Scene::new();
    let collapsed = scene.spawn();
    let parallel = scene.spawn();
    let sheared = scene.spawn();
    let mirrored = scene.spawn();
    scene
        .set_local_scale(collapsed, Vec3::new(1.0, 1.0, 0.0))
        .unwrap();
    scene
        .insert(
            collapsed,
            Camera {
                fov_y: 1.0,
                near: 0.1,
                far: 10.0,
            },
        )
        .unwrap();
    scene
        .insert(
            collapsed,
            Light::Directional {
                color: Vec3::ONE,
                intensity: 1.0,
            },
        )
        .unwrap();
    scene
        .insert(
            parallel,
            Camera {
                fov_y: 1.0,
                near: 0.1,
                far: 10.0,
            },
        )
        .unwrap();
    scene
        .insert(
            parallel,
            Light::Directional {
                color: Vec3::ONE,
                intensity: 1.0,
            },
        )
        .unwrap();
    scene
        .insert(
            sheared,
            Camera {
                fov_y: 1.0,
                near: 0.1,
                far: 10.0,
            },
        )
        .unwrap();
    scene
        .insert(
            mirrored,
            Camera {
                fov_y: 1.2,
                near: 0.1,
                far: 10.0,
            },
        )
        .unwrap();
    scene
        .insert(
            mirrored,
            Light::Directional {
                color: Vec3::X,
                intensity: 4.0,
            },
        )
        .unwrap();
    scene.update();
    write_basis(
        &mut scene,
        parallel,
        Mat4::from_cols(glam::Vec4::X, glam::Vec4::Z, glam::Vec4::Z, glam::Vec4::W),
    );
    write_basis(
        &mut scene,
        sheared,
        Mat4::from_cols(
            glam::Vec4::X,
            glam::Vec4::new(0.5, 1.0, 0.0, 0.0),
            glam::Vec4::Z,
            glam::Vec4::W,
        ),
    );
    write_basis(
        &mut scene,
        mirrored,
        Mat4::from_scale(Vec3::new(-1.0, 1.0, 1.0)),
    );
    let mut extract = Extract::default();
    scene.extract_into(&mut extract, any_material);
    assert!(
        extract
            .cameras
            .iter()
            .all(|camera| camera.entity != collapsed)
    );
    assert!(extract.lights.iter().all(|light| light.entity != collapsed));
    assert!(
        extract
            .cameras
            .iter()
            .all(|camera| camera.entity != parallel)
    );
    assert!(extract.lights.iter().all(|light| light.entity != parallel));
    let sheared_camera = extract
        .cameras
        .iter()
        .find(|camera| camera.entity == sheared)
        .expect("sheared camera");
    let rebuilt = sheared_camera.y_axis.cross(sheared_camera.z_axis);
    close(sheared_camera.x_axis, rebuilt);
    assert!(sheared_camera.x_axis.dot(glam::Vec3::X).abs() < 0.99);
    let mirrored_camera = extract
        .cameras
        .iter()
        .find(|camera| camera.entity == mirrored)
        .expect("mirrored camera");
    close(
        mirrored_camera.x_axis,
        mirrored_camera.y_axis.cross(mirrored_camera.z_axis),
    );
    assert!(mirrored_camera.x_axis.dot(Vec3::X) > 0.0);
    assert!(extract.lights.iter().any(|light| light.entity == mirrored));
}

fn write_basis(scene: &mut Scene, entity: Entity, world: Mat4) {
    let dense = scene.dense(entity).unwrap();
    scene.world[dense] = world;
    scene.transforms_dirty = false;
}

#[test]
#[cfg(debug_assertions)]
#[should_panic]
fn extract_without_update_panics() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.update();
    scene.set_local_position(entity, Vec3::X).unwrap();
    scene.extract_into(&mut Extract::default(), any_material);
}

#[test]
fn component_mask_tracks_insert_remove_and_replace() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    assert_eq!(scene.components(entity).unwrap(), ComponentMask::EMPTY);
    assert!(!scene.has::<Name>(entity).unwrap());

    let name = Name::new("Root");
    assert_eq!(scene.insert(entity, name.clone()).unwrap(), None);
    assert!(scene.has::<Name>(entity).unwrap());
    assert_eq!(
        scene.components(entity).unwrap().iter().collect::<Vec<_>>(),
        vec![ComponentId::Name]
    );

    *scene.get_mut::<Name>(entity).unwrap().unwrap() = Name::new("Edited");
    assert!(scene.has::<Name>(entity).unwrap());
    assert!(
        scene
            .get::<Name>(entity)
            .unwrap()
            .is_some_and(|found| found == "Edited")
    );

    let replaced = Name::new("Next");
    assert_eq!(
        scene.insert(entity, replaced.clone()).unwrap().as_ref(),
        Some(&Name::new("Edited"))
    );
    assert!(scene.has::<Name>(entity).unwrap());
    assert_eq!(scene.get::<Name>(entity).unwrap(), Some(&replaced));
    assert_eq!(scene.remove::<Name>(entity).unwrap(), Some(replaced));
    assert!(!scene.has::<Name>(entity).unwrap());
    assert_eq!(scene.components(entity).unwrap(), ComponentMask::EMPTY);
    assert_eq!(scene.remove::<Name>(entity).unwrap(), None);
    assert!(!scene.has::<Name>(entity).unwrap());
}

#[test]
fn has_rejects_stale_entity() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.insert(entity, Name::new("Root")).unwrap();
    scene.despawn(entity).unwrap();
    assert!(matches!(scene.has::<Name>(entity), Err(Error::StaleEntity)));
    assert!(matches!(scene.components(entity), Err(Error::StaleEntity)));
}

#[test]
fn components_iter_matches_present_columns() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    let (mesh, camera, light) = sample_components();
    scene.insert(entity, Name::new("Root")).unwrap();
    scene.insert(entity, mesh).unwrap();
    scene.insert(entity, camera).unwrap();
    scene.insert(entity, light).unwrap();
    assert_eq!(
        scene.components(entity).unwrap().iter().collect::<Vec<_>>(),
        vec![
            ComponentId::Name,
            ComponentId::Mesh,
            ComponentId::Camera,
            ComponentId::Light,
        ]
    );
    assert!(scene.has::<Name>(entity).unwrap());
    assert!(scene.has::<MeshRenderer>(entity).unwrap());
    assert!(scene.has::<Camera>(entity).unwrap());
    assert!(scene.has::<Light>(entity).unwrap());
}

#[test]
fn despawn_leaves_absent_columns_and_respawn_is_empty() {
    let mut scene = Scene::new();
    let kept = scene.spawn();
    let dropped = scene.spawn();
    let mesh = MeshRenderer {
        model: Model::Primitive(Primitive::Cube),
        material: asset(1),
    };
    scene.insert(kept, mesh).unwrap();
    scene.insert(dropped, Name::new("Gone")).unwrap();
    scene.despawn(dropped).unwrap();
    assert!(scene.iter::<Name>().next().is_none());
    assert_eq!(
        scene.iter::<MeshRenderer>().collect::<Vec<_>>(),
        vec![(kept, &mesh)]
    );
    assert!(scene.iter::<Camera>().next().is_none());
    assert!(scene.iter::<Light>().next().is_none());
    let reused = scene.spawn();
    assert_eq!(reused.index(), dropped.index());
    assert_eq!(scene.components(reused).unwrap(), ComponentMask::EMPTY);
    assert!(!scene.has::<Name>(reused).unwrap());
    assert!(matches!(
        scene.has::<Name>(dropped),
        Err(Error::StaleEntity)
    ));
}

#[test]
fn iter_mut_updates_component_in_place() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.insert(entity, Name::new("Root")).unwrap();
    for (found, name) in scene.iter_mut::<Name>() {
        assert_eq!(found, entity);
        *name = Name::new("Renamed");
    }
    assert!(scene.has::<Name>(entity).unwrap());
    assert!(
        scene
            .get::<Name>(entity)
            .unwrap()
            .is_some_and(|name| name == "Renamed")
    );
}
