use std::sync::OnceLock;

use glam::{Mat4, Vec3, Vec4};
use render::primitive::{CpuMesh, build};
use scene::{Entity, MeshRenderer, Model, Primitive, Scene};

const PARALLEL: f32 = 1.0e-6;
const DEGENERATE: f32 = 1.0e-8;

struct Ray {
    origin: Vec3,
    direction: Vec3,
}

pub(crate) fn pick(
    camera: &render::Camera,
    viewport: egui::Rect,
    pos: egui::Pos2,
    scene: &mut Scene,
) -> Option<Entity> {
    let ray = ray_from_viewport(camera, viewport, pos)?;
    scene.update();
    let mut best: Option<(f32, Entity)> = None;
    for (entity, mesh) in scene.iter::<MeshRenderer>() {
        let Ok(world) = scene.world(entity) else {
            continue;
        };
        let Model::Primitive(primitive) = mesh.model;
        let Some(distance) = distance_to(&ray, world, primitive_mesh(primitive)) else {
            continue;
        };
        if best.is_none_or(|(nearest, _)| distance < nearest) {
            best = Some((distance, entity));
        }
    }
    best.map(|(_, entity)| entity)
}

fn ray_from_viewport(
    camera: &render::Camera,
    viewport: egui::Rect,
    pos: egui::Pos2,
) -> Option<Ray> {
    let width = viewport.width();
    let height = viewport.height();
    if width < 1.0 || height < 1.0 {
        return None;
    }
    let ndc_x = (pos.x - viewport.min.x) / width * 2.0 - 1.0;
    let ndc_y = 1.0 - (pos.y - viewport.min.y) / height * 2.0;
    let view_proj = camera.projection(viewport.aspect_ratio()) * camera.view();
    if view_proj.determinant().abs() < DEGENERATE {
        return None;
    }
    let hit = view_proj.inverse() * Vec4::new(ndc_x, ndc_y, 0.0, 1.0);
    if !hit.is_finite() || hit.w.abs() < DEGENERATE {
        return None;
    }
    let near = hit.truncate() / hit.w;
    let direction = near - camera.position();
    if !near.is_finite() || direction.length_squared() < DEGENERATE {
        return None;
    }
    Some(Ray {
        origin: camera.position(),
        direction: direction.normalize(),
    })
}

fn primitive_mesh(primitive: Primitive) -> &'static CpuMesh {
    &meshes()[primitive.kind() as usize]
}

fn meshes() -> &'static [CpuMesh; 7] {
    static MESHES: OnceLock<[CpuMesh; 7]> = OnceLock::new();
    MESHES.get_or_init(|| {
        std::array::from_fn(|kind| {
            build(Primitive::from_kind(kind as u32).expect("primitive kind"))
        })
    })
}

fn distance_to(ray: &Ray, world: Mat4, mesh: &CpuMesh) -> Option<f32> {
    if world.determinant().abs() < DEGENERATE {
        return None;
    }
    let inverse = world.inverse();
    let origin = inverse.transform_point3(ray.origin);
    let direction = inverse.transform_vector3(ray.direction);
    if !origin.is_finite() || !direction.is_finite() || direction.length_squared() < DEGENERATE {
        return None;
    }
    let mut nearest = None;
    for triangle in mesh.indices.chunks_exact(3) {
        let v0 = Vec3::from(mesh.vertices[triangle[0] as usize].position);
        let v1 = Vec3::from(mesh.vertices[triangle[1] as usize].position);
        let v2 = Vec3::from(mesh.vertices[triangle[2] as usize].position);
        let Some(distance) = ray_triangle(origin, direction, v0, v1, v2) else {
            continue;
        };
        if nearest.is_none_or(|nearest| distance < nearest) {
            nearest = Some(distance);
        }
    }
    nearest
}

fn ray_triangle(origin: Vec3, direction: Vec3, v0: Vec3, v1: Vec3, v2: Vec3) -> Option<f32> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let p = direction.cross(edge2);
    let det = edge1.dot(p);
    if det.abs() < PARALLEL {
        return None;
    }
    let inv_det = 1.0 / det;
    let tvec = origin - v0;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = tvec.cross(edge1);
    let v = direction.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let distance = edge2.dot(q) * inv_det;
    (distance > 0.0).then_some(distance)
}

#[cfg(test)]
mod tests {
    use scene::{AssetId, MeshRenderer, Model, Primitive, Quat, Scene, Vec3};

    use super::{pick, ray_from_viewport};

    fn viewport() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(4.0, 8.0), egui::vec2(320.0, 180.0))
    }

    fn camera() -> render::Camera {
        render::Camera::new()
    }

    fn mesh(primitive: Primitive) -> MeshRenderer {
        MeshRenderer {
            model: Model::Primitive(primitive),
            material: AssetId([0; 16]),
        }
    }

    fn spawn(scene: &mut Scene, position: Vec3, primitive: Primitive) -> scene::Entity {
        let entity = scene.spawn();
        scene.set_local_position(entity, position).unwrap();
        scene.insert(entity, mesh(primitive)).unwrap();
        entity
    }

    #[test]
    fn center_ray_starts_on_the_near_plane() {
        let camera = camera();
        let viewport = viewport();
        let ray = ray_from_viewport(&camera, viewport, viewport.center()).unwrap();
        let forward = (-camera.position()).normalize();
        let view_proj = camera.projection(viewport.aspect_ratio()) * camera.view();
        let hit = view_proj.inverse() * glam::Vec4::new(0.0, 0.0, 0.0, 1.0);
        let near = hit.truncate() / hit.w;
        let delta = near - camera.position();
        assert!(delta.dot(forward) > 0.0);
        assert!((delta.length() - 0.1).abs() < 1.0e-3);
        assert!((ray.direction - delta.normalize()).length() < 1.0e-4);
    }

    #[test]
    fn center_ray_hits_cube_at_origin_and_misses_elsewhere() {
        let camera = camera();
        let viewport = viewport();
        let mut scene = Scene::new();
        let cube = spawn(&mut scene, Vec3::ZERO, Primitive::Cube);
        let hit = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(hit, Some(cube));

        scene.set_local_position(cube, Vec3::Y * 8.0).unwrap();
        let miss = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(miss, None);
    }

    #[test]
    fn nearer_cube_wins() {
        let camera = camera();
        let viewport = viewport();
        let mut scene = Scene::new();
        let _far = spawn(&mut scene, Vec3::ZERO, Primitive::Cube);
        let near = spawn(&mut scene, camera.position() * 0.5, Primitive::Cube);
        let hit = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(hit, Some(near));
    }

    #[test]
    fn rotated_scaled_cube_still_hits() {
        let camera = camera();
        let viewport = viewport();
        let mut scene = Scene::new();
        let cube = spawn(&mut scene, Vec3::ZERO, Primitive::Cube);
        scene
            .set_local_rotation(cube, Quat::from_rotation_y(0.7))
            .unwrap();
        scene
            .set_local_scale(cube, Vec3::new(2.0, 0.4, 1.5))
            .unwrap();
        let hit = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(hit, Some(cube));
    }

    #[test]
    fn plane_hits_from_both_sides() {
        let camera = camera();
        let viewport = viewport();
        let mut scene = Scene::new();
        let plane = spawn(&mut scene, Vec3::ZERO, Primitive::Plane);
        let front = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(front, Some(plane));

        scene
            .set_local_rotation(plane, Quat::from_rotation_x(std::f32::consts::PI))
            .unwrap();
        let back = pick(&camera, viewport, viewport.center(), &mut scene);
        assert_eq!(back, Some(plane));
    }
}
