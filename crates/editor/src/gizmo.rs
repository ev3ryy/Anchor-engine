use transform_gizmo_egui::mint::{Quaternion, RowMatrix4, Vector3};
use transform_gizmo_egui::{
    Gizmo, GizmoConfig, GizmoInteraction, GizmoMode, GizmoOrientation, math::Transform,
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Translate,
    Rotate,
    Scale,
}

pub(crate) struct ObjectGizmo {
    inner: Gizmo,
    kind: Kind,
    orientation: GizmoOrientation,
    dragging: bool,
    target: Option<scene::Entity>,
}

impl Default for ObjectGizmo {
    fn default() -> Self {
        Self {
            inner: Gizmo::default(),
            kind: Kind::Translate,
            orientation: GizmoOrientation::Local,
            dragging: false,
            target: None,
        }
    }
}

impl ObjectGizmo {
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
        camera: &render::Camera,
        world: &mut scene::Scene,
        selection: &Option<scene::Entity>,
    ) {
        self.shortcuts(ui);
        let Some(entity) = *selection else {
            self.dragging = false;
            self.target = None;
            return;
        };
        let toolbar = toolbar_rect(viewport);
        if viewport.width() >= 1.0 && viewport.height() >= 1.0 {
            self.interact(ui, viewport, toolbar, camera, world, entity);
        }
        self.toolbar(ui, toolbar);
    }

    pub(crate) fn claims_pointer(
        &mut self,
        ui: &egui::Ui,
        viewport: egui::Rect,
        camera: &render::Camera,
        world: &mut scene::Scene,
        selection: &Option<scene::Entity>,
    ) -> bool {
        let Some(entity) = *selection else {
            return false;
        };
        let pointer = pointer_state(ui);
        let Some(pos) = pointer.pos else {
            return self.dragging;
        };
        if self.dragging || toolbar_rect(viewport).contains(pos) {
            return true;
        }
        if self.target != Some(entity) || viewport.width() < 1.0 || viewport.height() < 1.0 {
            return false;
        }
        world.update();
        let Ok(world_matrix) = world.world(entity) else {
            return false;
        };
        let Some(target) = gizmo_transform(world_matrix) else {
            return false;
        };
        self.aim(ui, viewport, camera, &pointer, target);
        viewport.contains(pos) && self.inner.pick_preview((pos.x, pos.y))
    }

    fn shortcuts(&mut self, ui: &mut egui::Ui) {
        if ui.ctx().text_edit_focused() {
            return;
        }
        let next = ui.input_mut(|input| {
            [
                (egui::Key::Num1, Kind::Translate),
                (egui::Key::Num2, Kind::Rotate),
                (egui::Key::Num3, Kind::Scale),
            ]
            .into_iter()
            .find(|(key, _)| input.consume_key(egui::Modifiers::NONE, *key))
            .map(|(_, kind)| kind)
        });
        if let Some(kind) = next
            && kind != self.kind
        {
            self.kind = kind;
            self.dragging = false;
        }
    }

    fn interact(
        &mut self,
        ui: &mut egui::Ui,
        viewport: egui::Rect,
        toolbar: egui::Rect,
        camera: &render::Camera,
        world: &mut scene::Scene,
        entity: scene::Entity,
    ) {
        let switched = self.target != Some(entity);
        if switched {
            self.inner = Gizmo::default();
            self.dragging = false;
            self.target = Some(entity);
        }
        world.update();
        let Ok(world_matrix) = world.world(entity) else {
            return;
        };
        let Some(target) = gizmo_transform(world_matrix) else {
            return;
        };
        let pointer = pointer_state(ui);
        if !pointer.primary {
            self.dragging = false;
        }
        let cursor = self.aim(ui, viewport, camera, &pointer, target);
        let over = !switched
            && pointer.pos.is_some_and(|pos| {
                viewport.contains(pos)
                    && !toolbar.contains(pos)
                    && self.inner.pick_preview((pos.x, pos.y))
            });
        let result = self.inner.update(
            GizmoInteraction {
                cursor_pos: (cursor.x, cursor.y),
                hovered: over,
                drag_started: pointer.pressed && over,
                dragging: pointer.primary && (self.dragging || (pointer.pressed && over)),
            },
            &[target],
        );
        self.dragging = pointer.primary && (result.is_some() || self.dragging);
        paint(ui, viewport, &self.inner);
        if let Some((_, transforms)) = result
            && let Some(transform) = transforms.first()
        {
            apply_world(world, entity, *transform);
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let mut overlay = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("gizmo-toolbar")
                .max_rect(rect),
        );
        egui::Frame::new()
            .fill(egui::Color32::from_black_alpha(180))
            .corner_radius(4)
            .inner_margin(4)
            .show(&mut overlay, |ui| {
                ui.horizontal(|ui| {
                    kind_button(ui, &mut self.kind, Kind::Translate, "Move");
                    kind_button(ui, &mut self.kind, Kind::Rotate, "Rotate");
                    kind_button(ui, &mut self.kind, Kind::Scale, "Scale");
                    ui.separator();
                    orientation_button(ui, &mut self.orientation, GizmoOrientation::Local, "Local");
                    orientation_button(
                        ui,
                        &mut self.orientation,
                        GizmoOrientation::Global,
                        "World",
                    );
                });
            });
    }
}

struct Pointer {
    pos: Option<egui::Pos2>,
    primary: bool,
    pressed: bool,
    snap: bool,
}

fn pointer_state(ui: &egui::Ui) -> Pointer {
    ui.input(|input| Pointer {
        pos: input.pointer.latest_pos(),
        primary: input.pointer.button_down(egui::PointerButton::Primary),
        pressed: input.pointer.button_pressed(egui::PointerButton::Primary),
        snap: input.modifiers.ctrl || input.modifiers.command,
    })
}

fn kind_button(ui: &mut egui::Ui, kind: &mut Kind, next: Kind, label: &str) {
    if ui.selectable_label(*kind == next, label).clicked() {
        *kind = next;
    }
}

fn orientation_button(
    ui: &mut egui::Ui,
    orientation: &mut GizmoOrientation,
    next: GizmoOrientation,
    label: &str,
) {
    if ui.selectable_label(*orientation == next, label).clicked() {
        *orientation = next;
    }
}

fn modes(kind: Kind) -> transform_gizmo_egui::EnumSet<GizmoMode> {
    match kind {
        Kind::Translate => GizmoMode::all_translate(),
        Kind::Rotate => GizmoMode::all_rotate(),
        Kind::Scale => GizmoMode::all_scale(),
    }
}

impl ObjectGizmo {
    fn aim(
        &mut self,
        ui: &egui::Ui,
        viewport: egui::Rect,
        camera: &render::Camera,
        pointer: &Pointer,
        target: Transform,
    ) -> egui::Pos2 {
        self.inner.update_config(GizmoConfig {
            view_matrix: row_matrix(camera.view()),
            projection_matrix: row_matrix(camera.projection(viewport.aspect_ratio())),
            viewport,
            modes: modes(self.kind),
            orientation: self.orientation,
            snapping: pointer.snap,
            pixels_per_point: ui.pixels_per_point(),
            ..*self.inner.config()
        });
        let cursor = pointer.pos.unwrap_or(viewport.center());
        if !self.dragging {
            let _ = self.inner.update(idle(cursor), &[target]);
        }
        cursor
    }
}

fn idle(cursor: egui::Pos2) -> GizmoInteraction {
    GizmoInteraction {
        cursor_pos: (cursor.x, cursor.y),
        hovered: false,
        drag_started: false,
        dragging: false,
    }
}

fn toolbar_rect(viewport: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_size(viewport.min + egui::vec2(8.0, 8.0), egui::vec2(360.0, 36.0))
        .intersect(viewport)
}

/// `transform-gizmo` flips NDC Y into egui space. The scene pass flips Y again for Vulkan,
/// so the gizmo uses the unflipped camera projection.
fn row_matrix(matrix: glam::Mat4) -> RowMatrix4<f64> {
    matrix.as_dmat4().into()
}

/// Zero (or near-zero) scale makes the matrix singular; glam then yields NaN rotation,
/// which crashes egui's tessellator when the gizmo mesh is painted.
fn gizmo_transform(matrix: scene::Mat4) -> Option<Transform> {
    let (scale, rotation, translation) = matrix.to_scale_rotation_translation();
    if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
        return None;
    }
    if scale.x.abs() < 1.0e-8 || scale.y.abs() < 1.0e-8 || scale.z.abs() < 1.0e-8 {
        return None;
    }
    Some(Transform::from_scale_rotation_translation(
        scale.as_dvec3(),
        rotation.as_dquat(),
        translation.as_dvec3(),
    ))
}

fn apply_world(world: &mut scene::Scene, entity: scene::Entity, transform: Transform) {
    let scale = vec3(transform.scale);
    let rotation = quat(transform.rotation);
    let translation = vec3(transform.translation);
    if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
        return;
    }
    let world_matrix = scene::Mat4::from_scale_rotation_translation(scale, rotation, translation);
    let parent = match world.parent(entity) {
        Ok(parent) => parent,
        Err(_) => return,
    };
    let parent_matrix = match parent {
        Some(parent) => match world.world(parent) {
            Ok(matrix) => matrix,
            Err(_) => return,
        },
        None => scene::Mat4::IDENTITY,
    };
    if parent_matrix.determinant().abs() < 1.0e-8 {
        return;
    }
    let local = parent_matrix.inverse() * world_matrix;
    let (scale, rotation, translation) = local.to_scale_rotation_translation();
    if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
        return;
    }
    let _ = world.set_local_scale(entity, scale);
    let _ = world.set_local_rotation(entity, rotation);
    let _ = world.set_local_position(entity, translation);
}

fn vec3(value: Vector3<f64>) -> scene::Vec3 {
    scene::Vec3::new(value.x as f32, value.y as f32, value.z as f32)
}

fn quat(value: Quaternion<f64>) -> scene::Quat {
    scene::Quat::from_xyzw(
        value.v.x as f32,
        value.v.y as f32,
        value.v.z as f32,
        value.s as f32,
    )
}

fn paint(ui: &egui::Ui, viewport: egui::Rect, gizmo: &Gizmo) {
    let draw = gizmo.draw();
    if draw.vertices.is_empty() {
        return;
    }
    if draw
        .vertices
        .iter()
        .any(|pos| !pos[0].is_finite() || !pos[1].is_finite())
    {
        return;
    }
    ui.painter().with_clip_rect(viewport).add(egui::Mesh {
        indices: draw.indices,
        vertices: draw
            .vertices
            .into_iter()
            .zip(draw.colors)
            .map(|(pos, [r, g, b, a])| egui::epaint::Vertex {
                pos: egui::pos2(pos[0], pos[1]),
                uv: egui::Pos2::ZERO,
                color: egui::Rgba::from_rgba_premultiplied(r, g, b, a).into(),
            })
            .collect(),
        ..Default::default()
    });
}

#[cfg(test)]
mod tests {
    use super::{apply_world, gizmo_transform};

    #[test]
    fn child_keeps_parent_when_world_moves() {
        let mut scene = scene::Scene::new();
        let parent = scene.spawn();
        scene
            .set_local_position(parent, scene::Vec3::X * 2.0)
            .expect("parent");
        scene
            .set_local_rotation(
                parent,
                scene::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            )
            .expect("parent");
        let child = scene.spawn_child(parent).expect("child");
        scene
            .set_local_position(child, scene::Vec3::X)
            .expect("child");
        scene.update();
        let (scale, rotation, translation) = scene
            .world(child)
            .expect("child")
            .to_scale_rotation_translation();
        let moved = gizmo_transform(scene::Mat4::from_scale_rotation_translation(
            scale,
            rotation,
            translation + scene::Vec3::Y,
        ))
        .expect("finite transform");
        apply_world(&mut scene, child, moved);
        scene.update();
        let got = scene.world(child).expect("child").w_axis.truncate();
        assert!((got - (translation + scene::Vec3::Y)).length() < 1.0e-4);
        assert_eq!(
            scene.local(parent).expect("parent").translation,
            scene::Vec3::X * 2.0
        );
    }

    #[test]
    fn zero_scale_is_rejected() {
        let matrix = scene::Mat4::from_scale_rotation_translation(
            scene::Vec3::new(1.0, 0.0, 1.0),
            scene::Quat::IDENTITY,
            scene::Vec3::ZERO,
        );
        assert!(gizmo_transform(matrix).is_none());
    }
}
