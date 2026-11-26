use crate::Error;
use scene::Draw;

pub(crate) const CAPACITY: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Batch {
    pub material_index: u32,
    pub begin: u32,
    pub count: u32,
    pub vertices: u32,
}

pub(crate) fn plan_draws(
    draws: &[Draw],
    mut resolve: impl FnMut(scene::MaterialRef) -> Option<u32>,
    order: &mut Vec<usize>,
    batches: &mut Vec<Batch>,
) -> Result<(), Error> {
    order.clear();
    batches.clear();
    for (index, draw) in draws.iter().enumerate() {
        if resolve(draw.material).is_some() {
            order.push(index);
        }
    }
    if order.len() > CAPACITY {
        order.clear();
        return Err(Error::OutOfInstances);
    }
    order.sort_by(|&left, &right| {
        draws[left]
            .mesh
            .cmp(&draws[right].mesh)
            .then(draws[left].material.cmp(&draws[right].material))
    });
    let mut cursor = 0;
    while cursor < order.len() {
        let draw = &draws[order[cursor]];
        let material_index = resolve(draw.material).expect("live material");
        let mesh = draw.mesh;
        let material = draw.material;
        let begin = cursor;
        cursor += 1;
        while cursor < order.len() {
            let next = &draws[order[cursor]];
            if next.mesh != mesh || next.material != material {
                break;
            }
            cursor += 1;
        }
        batches.push(Batch {
            material_index,
            begin: begin as u32,
            count: (cursor - begin) as u32,
            vertices: vertex_count(mesh),
        });
    }
    Ok(())
}

pub(crate) fn vertex_count(mesh: scene::Mesh) -> u32 {
    match mesh {
        scene::Mesh::Cube => 36,
    }
}

#[cfg(test)]
mod tests {
    use scene::{AssetId, Extract, MaterialRef, Mesh, MeshRenderer, Scene, Vec3};

    fn asset(index: u32, generation: u32) -> AssetId {
        let mut bytes = [0; 16];
        bytes[..4].copy_from_slice(&index.to_le_bytes());
        bytes[4..8].copy_from_slice(&generation.to_le_bytes());
        AssetId(bytes)
    }

    fn bind(asset: AssetId) -> Option<MaterialRef> {
        let index = u32::from_le_bytes(asset.0[..4].try_into().unwrap());
        let generation = u32::from_le_bytes(asset.0[4..8].try_into().unwrap());
        Some(MaterialRef { index, generation })
    }

    use super::plan_draws;
    use crate::material::MaterialStore;
    use crate::texture::SamplerId;
    use crate::{Error, MaterialId};

    fn draws(count: usize, stale_tail: usize) -> Vec<scene::Draw> {
        let mut scene = Scene::new();
        for index in 0..count {
            let entity = scene.spawn();
            let generation = if index >= count - stale_tail { 0 } else { 1 };
            scene
                .insert(
                    entity,
                    MeshRenderer {
                        mesh: Mesh::Cube,
                        material: asset(index as u32, generation),
                    },
                )
                .unwrap();
            scene
                .set_local_position(entity, Vec3::new(index as f32, 0.0, 0.0))
                .unwrap();
        }
        scene.update();
        let mut extract = Extract::default();
        scene.extract_into(&mut extract, bind);
        extract.draws
    }

    #[test]
    fn too_many_live_instances_is_an_error() {
        let draws = draws(257, 0);
        let mut order = Vec::new();
        let mut batches = Vec::new();
        let error = plan_draws(
            &draws,
            |material| Some(material.index),
            &mut order,
            &mut batches,
        );
        assert!(matches!(error, Err(Error::OutOfInstances)));
        assert!(order.is_empty());
    }

    #[test]
    fn stale_draws_do_not_count_toward_the_cap() {
        let draws = draws(257, 1);
        let mut order = Vec::new();
        let mut batches = Vec::new();
        plan_draws(
            &draws,
            |material| (material.generation != 0).then_some(material.index),
            &mut order,
            &mut batches,
        )
        .expect("256 live");
        assert_eq!(order.len(), 256);
        assert!(batches.iter().map(|batch| batch.count).sum::<u32>() == 256);
    }

    #[test]
    fn stale_generation_is_left_out_of_the_batch() {
        let mut store = MaterialStore::new(4);
        let stale = store
            .create(0, SamplerId::LinearRepeat, [1.0, 1.0, 1.0, 1.0])
            .expect("material");
        store.free(stale).unwrap();
        let live = store
            .create(0, SamplerId::LinearRepeat, [1.0, 1.0, 1.0, 1.0])
            .expect("material");
        let mut scene = Scene::new();
        let kept = scene.spawn();
        let dropped = scene.spawn();
        scene.set_local_position(kept, Vec3::Y).unwrap();
        scene.set_local_position(dropped, Vec3::X).unwrap();
        scene
            .insert(
                kept,
                MeshRenderer {
                    mesh: Mesh::Cube,
                    material: asset(live.index, live.generation),
                },
            )
            .unwrap();
        scene
            .insert(
                dropped,
                MeshRenderer {
                    mesh: Mesh::Cube,
                    material: asset(stale.index, stale.generation),
                },
            )
            .unwrap();
        scene.update();
        let mut extract = Extract::default();
        scene.extract_into(&mut extract, bind);
        let mut order = Vec::new();
        let mut batches = Vec::new();
        plan_draws(
            &extract.draws,
            |material| {
                store
                    .index(MaterialId {
                        index: material.index,
                        generation: material.generation,
                    })
                    .ok()
            },
            &mut order,
            &mut batches,
        )
        .unwrap();
        assert_eq!(order.len(), 1);
        assert_eq!(extract.draws[order[0]].entity, kept);
        assert_eq!(batches.len(), 1);
        assert_eq!(batches[0].material_index, live.index);
        assert_eq!(batches[0].begin, 0);
        assert_eq!(batches[0].count, 1);
    }

    #[test]
    fn worlds_follow_sorted_batch_order() {
        let mut scene = Scene::new();
        let later = scene.spawn();
        let earlier = scene.spawn();
        scene.set_local_position(later, Vec3::X).unwrap();
        scene.set_local_position(earlier, Vec3::Y).unwrap();
        scene
            .insert(
                later,
                MeshRenderer {
                    mesh: Mesh::Cube,
                    material: asset(4, 1),
                },
            )
            .unwrap();
        scene
            .insert(
                earlier,
                MeshRenderer {
                    mesh: Mesh::Cube,
                    material: asset(1, 1),
                },
            )
            .unwrap();
        scene.update();
        let mut extract = Extract::default();
        scene.extract_into(&mut extract, bind);
        let mut order = Vec::new();
        let mut batches = Vec::new();
        plan_draws(
            &extract.draws,
            |material| Some(material.index),
            &mut order,
            &mut batches,
        )
        .unwrap();
        assert_eq!(extract.draws[order[0]].entity, earlier);
        assert_eq!(extract.draws[order[1]].entity, later);
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].begin, 0);
        assert_eq!(batches[1].begin, 1);
        let worlds: Vec<_> = order
            .iter()
            .map(|&index| extract.draws[index].world)
            .collect();
        assert_eq!(worlds[0], scene.world(earlier).unwrap());
        assert_eq!(worlds[1], scene.world(later).unwrap());
    }
}
