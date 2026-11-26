use super::{Column, Entity, Error, Quat, Scene, Vec3};

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
    column.insert(entity, 7u32);
    assert_eq!(column.get(entity), Some(&7));
    assert_eq!(column.iter().collect::<Vec<_>>(), vec![(entity, &7)]);

    scene.despawn(entity).unwrap();
    let reused = scene.spawn();
    assert_eq!(reused.index(), entity.index());
    assert!(column.get(reused).is_none());
    assert_eq!(column.get(entity), Some(&7));

    column.insert(reused, 9);
    assert_eq!(column.get(reused), Some(&9));
    assert!(column.get(entity).is_none());
    assert_eq!(column.remove(reused), Some(9));
    assert!(column.get(reused).is_none());
}

#[test]
fn local_is_readable_while_dirty() {
    let mut scene = Scene::new();
    let entity = scene.spawn();
    scene.set_name(entity, "Root").unwrap();
    scene.update();
    scene.set_local_position(entity, Vec3::X).unwrap();
    assert_eq!(scene.local(entity).unwrap().translation, Vec3::X);
    assert_eq!(scene.name(entity).unwrap(), Some("Root"));
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
