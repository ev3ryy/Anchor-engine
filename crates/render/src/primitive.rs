use ash::vk;

use scene::{Model, Primitive};

use crate::Error;
use crate::gpu::GpuBuffer;

const SEGMENTS: u32 = 32;
const SPHERE_RINGS: u32 = 16;
const HEMI_RINGS: u32 = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Clone, Debug, PartialEq)]
pub struct CpuMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

pub fn build(primitive: Primitive) -> CpuMesh {
    match primitive {
        Primitive::Cube => cube(),
        Primitive::Sphere => sphere(),
        Primitive::Cylinder => cylinder(),
        Primitive::Capsule => capsule(),
        Primitive::Plane => plane(),
        Primitive::Cone => cone(),
        Primitive::Wedge => wedge(),
    }
}

#[cfg(test)]
pub(crate) fn index_count(model: Model) -> u32 {
    let Model::Primitive(primitive) = model;
    build(primitive).indices.len() as u32
}

pub(crate) fn slot(model: Model) -> u32 {
    let Model::Primitive(primitive) = model;
    primitive.kind()
}

#[derive(Clone, Copy)]
pub(crate) struct DrawMesh {
    pub vertex: vk::Buffer,
    pub index: vk::Buffer,
}

struct Slot {
    vertices: GpuBuffer,
    indices: GpuBuffer,
    index_count: u32,
}

pub(crate) struct Cache {
    slots: [Slot; 7],
}

impl Cache {
    pub(crate) fn create(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
    ) -> Result<Self, Error> {
        let mut slots = Vec::with_capacity(Primitive::ALL.len());
        for primitive in Primitive::ALL {
            slots.push(Slot::upload(instance, physical_device, device, primitive)?);
        }
        let slots = slots
            .try_into()
            .unwrap_or_else(|_| panic!("primitive cache has one slot per primitive"));
        Ok(Self { slots })
    }

    pub(crate) fn index_counts(&self) -> [u32; 7] {
        std::array::from_fn(|index| self.slots[index].index_count)
    }

    pub(crate) fn draw_meshes(&self) -> [DrawMesh; 7] {
        std::array::from_fn(|index| DrawMesh {
            vertex: self.slots[index].vertices.buffer,
            index: self.slots[index].indices.buffer,
        })
    }
}

impl Slot {
    fn upload(
        instance: &ash::Instance,
        physical_device: vk::PhysicalDevice,
        device: &ash::Device,
        primitive: Primitive,
    ) -> Result<Self, Error> {
        let mesh = build(primitive);
        let vertex_bytes = vertex_bytes(&mesh.vertices);
        let index_bytes = index_bytes(&mesh.indices);
        let vertices = GpuBuffer::host_visible(
            instance,
            physical_device,
            device,
            vertex_bytes.len() as vk::DeviceSize,
            vk::BufferUsageFlags::VERTEX_BUFFER,
        )?;
        let indices = GpuBuffer::host_visible(
            instance,
            physical_device,
            device,
            index_bytes.len() as vk::DeviceSize,
            vk::BufferUsageFlags::INDEX_BUFFER,
        )?;
        unsafe {
            std::ptr::copy_nonoverlapping(
                vertex_bytes.as_ptr(),
                vertices.mapped(),
                vertex_bytes.len(),
            );
            std::ptr::copy_nonoverlapping(
                index_bytes.as_ptr(),
                indices.mapped(),
                index_bytes.len(),
            );
        }
        Ok(Self {
            vertices,
            indices,
            index_count: mesh.indices.len() as u32,
        })
    }
}

fn vertex_bytes(vertices: &[Vertex]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(vertices.len() * 32);
    for vertex in vertices {
        for value in vertex.position {
            bytes.extend(value.to_le_bytes());
        }
        for value in vertex.normal {
            bytes.extend(value.to_le_bytes());
        }
        for value in vertex.uv {
            bytes.extend(value.to_le_bytes());
        }
    }
    bytes
}

fn index_bytes(indices: &[u32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(indices.len() * 4);
    for index in indices {
        bytes.extend(index.to_le_bytes());
    }
    bytes
}

fn cube() -> CpuMesh {
    let mut mesh = CpuMesh::default();
    // Corners run counter-clockwise when looking at the face from outside.
    push_face(
        &mut mesh,
        [0.0, 0.0, 1.0],
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
    );
    push_face(
        &mut mesh,
        [0.0, 0.0, -1.0],
        [
            [0.5, -0.5, -0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [0.5, 0.5, -0.5],
        ],
    );
    push_face(
        &mut mesh,
        [0.0, 1.0, 0.0],
        [
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        ],
    );
    push_face(
        &mut mesh,
        [0.0, -1.0, 0.0],
        [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        ],
    );
    push_face(
        &mut mesh,
        [1.0, 0.0, 0.0],
        [
            [0.5, -0.5, 0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [0.5, 0.5, 0.5],
        ],
    );
    push_face(
        &mut mesh,
        [-1.0, 0.0, 0.0],
        [
            [-0.5, -0.5, -0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [-0.5, 0.5, -0.5],
        ],
    );
    mesh
}

fn plane() -> CpuMesh {
    let mut mesh = CpuMesh::default();
    push_face(
        &mut mesh,
        [0.0, 1.0, 0.0],
        [
            [-0.5, 0.0, 0.5],
            [0.5, 0.0, 0.5],
            [0.5, 0.0, -0.5],
            [-0.5, 0.0, -0.5],
        ],
    );
    mesh
}

fn wedge() -> CpuMesh {
    let mut mesh = CpuMesh::default();
    let slope = normalize([0.0, 1.0, -1.0]);
    push_face(
        &mut mesh,
        slope,
        [
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, -0.5, -0.5],
        ],
    );
    push_face(
        &mut mesh,
        [0.0, -1.0, 0.0],
        [
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        ],
    );
    push_face(
        &mut mesh,
        [0.0, 0.0, 1.0],
        [
            [-0.5, -0.5, 0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [-0.5, 0.5, 0.5],
        ],
    );
    push_triangle(
        &mut mesh,
        [1.0, 0.0, 0.0],
        [[0.5, -0.5, -0.5], [0.5, 0.5, 0.5], [0.5, -0.5, 0.5]],
    );
    push_triangle(
        &mut mesh,
        [-1.0, 0.0, 0.0],
        [[-0.5, -0.5, -0.5], [-0.5, -0.5, 0.5], [-0.5, 0.5, 0.5]],
    );
    mesh
}

fn sphere() -> CpuMesh {
    lathe(0.0, 0.0, SPHERE_RINGS, std::f32::consts::PI, 0.0, 1.0, None)
}

fn cylinder() -> CpuMesh {
    let mut mesh = lathe(0.0, 0.5, 1, 0.0, 1.0, 0.0, Some((0.5, -0.5)));
    push_cap(&mut mesh, 0.5, [0.0, 1.0, 0.0], true);
    push_cap(&mut mesh, -0.5, [0.0, -1.0, 0.0], false);
    mesh
}

fn cone() -> CpuMesh {
    let mut mesh = CpuMesh::default();
    let stride = SEGMENTS + 1;
    for j in 0..stride {
        let u = j as f32 / SEGMENTS as f32;
        let theta = u * std::f32::consts::TAU;
        mesh.vertices.push(Vertex {
            position: ring(0.5, -0.5, theta),
            normal: cone_normal(theta),
            uv: [u, 0.0],
        });
    }
    for j in 0..SEGMENTS {
        let u = j as f32 / SEGMENTS as f32;
        let theta = u * std::f32::consts::TAU;
        mesh.vertices.push(Vertex {
            position: [0.0, 0.5, 0.0],
            normal: cone_normal(theta),
            uv: [u, 1.0],
        });
        mesh.indices.extend([stride + j, j, j + 1]);
    }
    push_cap(&mut mesh, -0.5, [0.0, -1.0, 0.0], false);
    mesh
}

fn capsule() -> CpuMesh {
    let mut mesh = CpuMesh::default();
    // North pole v = 1, north equator v = 9/17, south equator v = 8/17, south pole v = 0.
    push_hemisphere(&mut mesh, 0.5, true, 1.0, 9.0 / 17.0);
    let north_equator = HEMI_RINGS * (SEGMENTS + 1);
    let south_equator = mesh.vertices.len() as u32;
    push_hemisphere(&mut mesh, -0.5, false, 8.0 / 17.0, 0.0);
    stitch(&mut mesh, north_equator, south_equator);
    mesh
}

/// Latitude sweep. `y_center` is the sphere center; a cylinder uses `fixed_radius`.
fn lathe(
    y_center: f32,
    fixed_radius: f32,
    rings: u32,
    phi_span: f32,
    v0: f32,
    v1: f32,
    y_ends: Option<(f32, f32)>,
) -> CpuMesh {
    let mut mesh = CpuMesh::default();
    let stride = SEGMENTS + 1;
    for i in 0..=rings {
        let t = i as f32 / rings as f32;
        let phi = t * phi_span;
        let (y, radius, normal_y) = if let Some((y_top, y_bottom)) = y_ends {
            let y = y_top + (y_bottom - y_top) * t;
            (y, fixed_radius, 0.0)
        } else {
            let y = y_center + 0.5 * phi.cos();
            (y, 0.5 * phi.sin(), y - y_center)
        };
        let v = v0 + (v1 - v0) * t;
        for j in 0..stride {
            let u = j as f32 / SEGMENTS as f32;
            let theta = u * std::f32::consts::TAU;
            let position = ring(radius, y, theta);
            mesh.vertices.push(Vertex {
                position,
                normal: normalize([position[0], normal_y, position[2]]),
                uv: [u, v],
            });
        }
    }
    for i in 0..rings {
        let row = i * stride;
        stitch(&mut mesh, row, row + stride);
    }
    mesh
}

fn push_hemisphere(mesh: &mut CpuMesh, center_y: f32, north: bool, v_start: f32, v_end: f32) {
    let stride = SEGMENTS + 1;
    let first = mesh.vertices.len() as u32;
    for i in 0..=HEMI_RINGS {
        let t = i as f32 / HEMI_RINGS as f32;
        let phi = t * std::f32::consts::FRAC_PI_2;
        let (y, radius) = if north {
            (center_y + 0.5 * phi.cos(), 0.5 * phi.sin())
        } else {
            (center_y - 0.5 * phi.sin(), 0.5 * phi.cos())
        };
        let v = v_start + (v_end - v_start) * t;
        for j in 0..stride {
            let u = j as f32 / SEGMENTS as f32;
            let theta = u * std::f32::consts::TAU;
            let position = ring(radius, y, theta);
            mesh.vertices.push(Vertex {
                position,
                normal: normalize([position[0], y - center_y, position[2]]),
                uv: [u, v],
            });
        }
    }
    for i in 0..HEMI_RINGS {
        stitch(mesh, first + i * stride, first + (i + 1) * stride);
    }
}

fn stitch(mesh: &mut CpuMesh, upper: u32, lower: u32) {
    for j in 0..SEGMENTS {
        let a = upper + j;
        let c = lower + j;
        mesh.indices.extend([a, c, c + 1, a, c + 1, a + 1]);
    }
}

fn push_cap(mesh: &mut CpuMesh, y: f32, normal: [f32; 3], top: bool) {
    let center = mesh.vertices.len() as u32;
    mesh.vertices.push(Vertex {
        position: [0.0, y, 0.0],
        normal,
        uv: [0.5, 0.5],
    });
    let ring_start = center + 1;
    for j in 0..=SEGMENTS {
        let u = j as f32 / SEGMENTS as f32;
        let theta = u * std::f32::consts::TAU;
        let position = ring(0.5, y, theta);
        mesh.vertices.push(Vertex {
            position,
            normal,
            uv: [position[0] + 0.5, position[2] + 0.5],
        });
    }
    for j in 0..SEGMENTS {
        if top {
            mesh.indices
                .extend([center, ring_start + j, ring_start + j + 1]);
        } else {
            mesh.indices
                .extend([center, ring_start + j + 1, ring_start + j]);
        }
    }
}

fn push_face(mesh: &mut CpuMesh, normal: [f32; 3], corners: [[f32; 3]; 4]) {
    let base = mesh.vertices.len() as u32;
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    for (position, uv) in corners.into_iter().zip(uvs) {
        mesh.vertices.push(Vertex {
            position,
            normal,
            uv,
        });
    }
    mesh.indices
        .extend([base, base + 2, base + 3, base, base + 1, base + 2]);
}

fn push_triangle(mesh: &mut CpuMesh, normal: [f32; 3], corners: [[f32; 3]; 3]) {
    let base = mesh.vertices.len() as u32;
    let uvs = [[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]];
    for (position, uv) in corners.into_iter().zip(uvs) {
        mesh.vertices.push(Vertex {
            position,
            normal,
            uv,
        });
    }
    mesh.indices.extend([base, base + 1, base + 2]);
}

fn ring(radius: f32, y: f32, theta: f32) -> [f32; 3] {
    [radius * theta.sin(), y, radius * theta.cos()]
}

fn cone_normal(theta: f32) -> [f32; 3] {
    normalize([theta.sin(), 0.5, theta.cos()])
}

fn normalize(value: [f32; 3]) -> [f32; 3] {
    let length = (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt();
    [value[0] / length, value[1] / length, value[2] / length]
}

impl Default for CpuMesh {
    fn default() -> Self {
        Self {
            vertices: Vec::new(),
            indices: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn length(value: [f32; 3]) -> f32 {
        (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
    }

    fn assert_indexed(mesh: &CpuMesh) {
        assert!(mesh.indices.len() % 3 == 0 && !mesh.indices.is_empty());
        for index in &mesh.indices {
            assert!((*index as usize) < mesh.vertices.len());
        }
        for vertex in &mesh.vertices {
            assert!(length(vertex.normal).is_finite());
            assert!((length(vertex.normal) - 1.0).abs() < 1e-4);
            assert!(vertex.position.iter().all(|value| value.is_finite()));
        }
    }

    fn assert_outward(mesh: &CpuMesh) {
        for triangle in mesh.indices.chunks(3) {
            let a = mesh.vertices[triangle[0] as usize].position;
            let b = mesh.vertices[triangle[1] as usize].position;
            let c = mesh.vertices[triangle[2] as usize].position;
            let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
            let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
            let face = [
                ab[1] * ac[2] - ab[2] * ac[1],
                ab[2] * ac[0] - ab[0] * ac[2],
                ab[0] * ac[1] - ab[1] * ac[0],
            ];
            if length(face) < 1e-6 {
                continue;
            }
            let face = normalize(face);
            let mut normal = [0.0; 3];
            for index in triangle {
                let vertex = mesh.vertices[*index as usize].normal;
                normal[0] += vertex[0];
                normal[1] += vertex[1];
                normal[2] += vertex[2];
            }
            let dot = face[0] * normal[0] + face[1] * normal[1] + face[2] * normal[2];
            assert!(dot > 0.5, "face {face:?} against normals {normal:?}");
        }
    }

    #[test]
    fn primitives_are_closed_outward_and_unit() {
        for primitive in Primitive::ALL {
            let mesh = build(primitive);
            assert_indexed(&mesh);
            assert_outward(&mesh);
        }
    }

    #[test]
    fn cube_is_flat_and_keeps_the_positive_z_winding() {
        let mesh = build(Primitive::Cube);
        assert_eq!(mesh.vertices.len(), 24);
        assert_eq!(mesh.indices.len(), 36);
        for face in mesh.vertices.chunks(4) {
            let normal = face[0].normal;
            assert!(face.iter().all(|vertex| vertex.normal == normal));
            let axes = normal
                .iter()
                .filter(|component| component.abs() > 0.9)
                .count();
            assert_eq!(axes, 1);
        }
        let a = mesh.vertices[0].position;
        let c = mesh.vertices[2].position;
        let d = mesh.vertices[3].position;
        assert_eq!(a, [-0.5, -0.5, 0.5]);
        assert_eq!(c, [0.5, 0.5, 0.5]);
        assert_eq!(d, [-0.5, 0.5, 0.5]);
        let cross = (c[0] - a[0]) * (d[1] - a[1]) - (d[0] - a[0]) * (c[1] - a[1]);
        assert!(cross > 0.0);
    }

    #[test]
    fn cylinder_rim_has_three_normals() {
        let mesh = build(Primitive::Cylinder);
        let mut top = false;
        let mut bottom = false;
        let mut side = false;
        for vertex in &mesh.vertices {
            let radial = (vertex.position[0] * vertex.position[0]
                + vertex.position[2] * vertex.position[2])
                .sqrt();
            if (radial - 0.5).abs() > 1e-4 || (vertex.position[1].abs() - 0.5).abs() > 1e-4 {
                continue;
            }
            if vertex.normal[1] > 0.9 {
                top = true;
            } else if vertex.normal[1] < -0.9 {
                bottom = true;
            } else if (vertex.normal[0] * vertex.normal[0] + vertex.normal[2] * vertex.normal[2])
                .sqrt()
                > 0.9
            {
                side = true;
            }
        }
        assert!(top && bottom && side);
    }

    #[test]
    fn capsule_is_two_units_tall_with_a_unit_body() {
        let mesh = build(Primitive::Capsule);
        let mut min_y = f32::MAX;
        let mut max_y = f32::MIN;
        for vertex in &mesh.vertices {
            min_y = min_y.min(vertex.position[1]);
            max_y = max_y.max(vertex.position[1]);
            if vertex.normal[1].abs() < 1e-4 {
                assert!((vertex.position[1].abs() - 0.5).abs() < 1e-4);
                let radial = (vertex.position[0] * vertex.position[0]
                    + vertex.position[2] * vertex.position[2])
                    .sqrt();
                assert!((radial - 0.5).abs() < 1e-4);
            }
        }
        assert!((min_y + 1.0).abs() < 1e-4);
        assert!((max_y - 1.0).abs() < 1e-4);
    }
}
