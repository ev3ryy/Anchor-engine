use egui_dock::{DockState, NodeIndex, TabViewer};
use render::Camera;

use crate::physical_pixels;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Hierarchy,
    Viewport,
    Inspector,
    Stats,
}

pub(crate) struct Panels<'a> {
    pub viewport: &'a mut Option<egui::Rect>,
    pub viewport_px: &'a mut [u32; 2],
    pub scene: Option<egui::TextureId>,
    pub world: &'a mut scene::Scene,
    pub selection: &'a mut Option<scene::Entity>,
    pub gizmo: &'a mut crate::gizmo::ObjectGizmo,
    pub default_material: scene::AssetId,
    pub camera: &'a Camera,
    pub dt: f32,
    pub extent: [u32; 2],
}

#[derive(Clone, Copy)]
enum HierarchyAction {
    CreateRoot,
    CreateChild(scene::Entity),
    Delete(scene::Entity),
}

pub(crate) fn dock_state() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Viewport]);
    let surface = dock.main_surface_mut();
    surface.split_left(NodeIndex::root(), 0.78, vec![Tab::Hierarchy]);
    surface.split_right(NodeIndex::root(), 0.78, vec![Tab::Inspector]);
    surface.split_below(NodeIndex::root(), 0.72, vec![Tab::Stats]);
    dock
}

impl TabViewer for Panels<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Self::Tab) -> egui::Id {
        egui::Id::new(*tab as u8)
    }

    fn title(&mut self, tab: &mut Self::Tab) -> egui::WidgetText {
        match tab {
            Tab::Hierarchy => "Hierarchy",
            Tab::Viewport => "Viewport",
            Tab::Inspector => "Inspector",
            Tab::Stats => "Stats",
        }
        .into()
    }

    fn ui(&mut self, ui: &mut egui::Ui, tab: &mut Self::Tab) {
        match tab {
            Tab::Hierarchy => {
                let action = hierarchy_panel(ui, self.world, self.selection);
                apply_hierarchy(self.world, self.selection, action);
            }
            Tab::Viewport => {
                let rect = ui.max_rect();
                *self.viewport = Some(rect);
                *self.viewport_px = physical_pixels(rect, ui.pixels_per_point());
                if let Some(texture) = self.scene {
                    ui.painter().image(
                        texture,
                        rect,
                        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                        egui::Color32::WHITE,
                    );
                }
                self.gizmo
                    .show(ui, rect, self.camera, self.world, self.selection);
            }
            Tab::Inspector => {
                inspector(ui, self.world, self.selection, self.default_material);
            }
            Tab::Stats => {
                let position = self.camera.position();
                ui.label(format!("frame     {:>7.2} ms", self.dt * 1000.0));
                ui.label(format!(
                    "position  {:>7.2} {:>7.2} {:>7.2}",
                    position.x, position.y, position.z
                ));
                ui.label(format!(
                    "yaw       {:>7.1}°",
                    self.camera.yaw().to_degrees()
                ));
                ui.label(format!(
                    "pitch     {:>7.1}°",
                    self.camera.pitch().to_degrees()
                ));
                ui.label(format!("speed     {:>7.2}", self.camera.speed()));
                ui.label(format!(
                    "viewport  {:>7} × {}",
                    self.viewport_px[0], self.viewport_px[1]
                ));
                ui.label(format!(
                    "window    {:>7} × {}",
                    self.extent[0], self.extent[1]
                ));
            }
        }
    }

    fn is_closeable(&self, _tab: &Self::Tab) -> bool {
        false
    }

    fn clear_background(&self, tab: &Self::Tab) -> bool {
        !matches!(tab, Tab::Viewport)
    }

    fn scroll_bars(&self, tab: &Self::Tab) -> [bool; 2] {
        if matches!(tab, Tab::Viewport) {
            [false, false]
        } else {
            [true, true]
        }
    }
}

fn hierarchy_panel(
    ui: &mut egui::Ui,
    world: &scene::Scene,
    selection: &mut Option<scene::Entity>,
) -> Option<HierarchyAction> {
    let mut action = None;
    ui.horizontal(|ui| {
        if ui.button("Create").clicked() {
            action = Some(HierarchyAction::CreateRoot);
        }
        let child = ui.add_enabled(selection.is_some(), egui::Button::new("Child"));
        if child.clicked()
            && let Some(parent) = *selection
        {
            action = Some(HierarchyAction::CreateChild(parent));
        }
        let delete = ui.add_enabled(selection.is_some(), egui::Button::new("Delete"));
        if delete.clicked()
            && let Some(entity) = *selection
        {
            action = Some(HierarchyAction::Delete(entity));
        }
    });
    if world.is_empty() {
        ui.label("No scene");
        return action;
    }
    let roots: Vec<_> = world.roots().collect();
    for root in roots {
        hierarchy_node(ui, world, selection, &mut action, root, 0);
    }
    action
}

fn hierarchy_node(
    ui: &mut egui::Ui,
    world: &scene::Scene,
    selection: &mut Option<scene::Entity>,
    action: &mut Option<HierarchyAction>,
    entity: scene::Entity,
    depth: usize,
) {
    ui.push_id(entity, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(depth as f32 * 12.0);
            let label = world
                .get::<scene::Name>(entity)
                .ok()
                .flatten()
                .map(scene::Name::as_str)
                .unwrap_or("Entity");
            let response = ui.selectable_label(*selection == Some(entity), label);
            if response.clicked() {
                *selection = Some(entity);
            }
            response.context_menu(|ui| {
                if ui.button("Child").clicked() {
                    *action = Some(HierarchyAction::CreateChild(entity));
                    ui.close();
                }
                if ui.button("Delete").clicked() {
                    *action = Some(HierarchyAction::Delete(entity));
                    ui.close();
                }
            });
        });
    });
    let children: Vec<_> = world
        .children(entity)
        .map(|kids| kids.collect())
        .unwrap_or_default();
    for child in children {
        hierarchy_node(ui, world, selection, action, child, depth + 1);
    }
}

fn apply_hierarchy(
    world: &mut scene::Scene,
    selection: &mut Option<scene::Entity>,
    action: Option<HierarchyAction>,
) {
    let Some(action) = action else {
        return;
    };
    match action {
        HierarchyAction::CreateRoot => {
            let entity = world.spawn();
            let _ = world.insert(entity, scene::Name::new("Entity"));
            *selection = Some(entity);
        }
        HierarchyAction::CreateChild(parent) => {
            if let Ok(entity) = world.spawn_child(parent) {
                let _ = world.insert(entity, scene::Name::new("Entity"));
                *selection = Some(entity);
            }
        }
        HierarchyAction::Delete(entity) => {
            if let Ok(destroyed) = world.despawn(entity)
                && selection.is_some_and(|selected| destroyed.contains(&selected))
            {
                *selection = None;
            }
        }
    }
}

fn inspector(
    ui: &mut egui::Ui,
    world: &mut scene::Scene,
    selection: &Option<scene::Entity>,
    default_material: scene::AssetId,
) {
    let Some(entity) = *selection else {
        ui.label("No selection");
        return;
    };
    if world.local(entity).is_err() {
        ui.label("No selection");
        return;
    }
    ui.label("Transform");
    edit_transform(ui, world, entity);
    let ids: Vec<_> = world
        .components(entity)
        .map(|mask| mask.iter().collect())
        .unwrap_or_default();
    for id in ids {
        ui.separator();
        let mut removed = false;
        ui.horizontal(|ui| {
            ui.label(component_label(id));
            removed = ui.button("Remove").clicked();
        });
        if removed {
            remove_component(world, entity, id);
            continue;
        }
        edit_component(ui, world, entity, id);
    }
    ui.separator();
    let mask = world
        .components(entity)
        .unwrap_or(scene::ComponentMask::EMPTY);
    let missing = [
        scene::ComponentId::Name,
        scene::ComponentId::Mesh,
        scene::ComponentId::Camera,
        scene::ComponentId::Light,
    ]
    .into_iter()
    .filter(|id| !mask.contains(*id))
    .collect::<Vec<_>>();
    if missing.is_empty() {
        ui.add_enabled(false, egui::Button::new("Add Component"));
        return;
    }
    ui.menu_button("Add Component", |ui| {
        for id in missing {
            if ui.button(component_label(id)).clicked() {
                add_component(world, entity, id, default_material);
                ui.close();
            }
        }
    });
}

fn edit_transform(ui: &mut egui::Ui, world: &mut scene::Scene, entity: scene::Entity) {
    let Ok(local) = world.local(entity) else {
        return;
    };
    let mut translation = local.translation;
    if drag_vec3(ui, "Position", &mut translation) {
        let _ = world.set_local_position(entity, translation);
    }
    let (yaw, pitch, roll) = local.rotation.to_euler(glam::EulerRot::YXZ);
    let mut degrees = [yaw.to_degrees(), pitch.to_degrees(), roll.to_degrees()];
    if drag_euler(ui, &mut degrees) {
        let rotation = scene::Quat::from_euler(
            glam::EulerRot::YXZ,
            degrees[0].to_radians(),
            degrees[1].to_radians(),
            degrees[2].to_radians(),
        );
        let _ = world.set_local_rotation(entity, rotation);
    }
    let mut scale = local.scale;
    if drag_vec3(ui, "Scale", &mut scale) {
        let _ = world.set_local_scale(entity, scale);
    }
}

fn edit_component(
    ui: &mut egui::Ui,
    world: &mut scene::Scene,
    entity: scene::Entity,
    id: scene::ComponentId,
) {
    match id {
        scene::ComponentId::Name => {
            let Some(name) = world
                .get::<scene::Name>(entity)
                .ok()
                .flatten()
                .map(scene::Name::as_str)
                .map(str::to_string)
            else {
                return;
            };
            let mut text = name;
            if ui.text_edit_singleline(&mut text).changed() {
                let _ = world.insert(entity, scene::Name::new(text));
            }
        }
        scene::ComponentId::Mesh => {
            let Some(mesh) = world
                .get::<scene::MeshRenderer>(entity)
                .ok()
                .flatten()
                .copied()
            else {
                return;
            };
            let mesh = edit_model(ui, world, entity, mesh);
            edit_material(ui, world, entity, mesh);
        }
        scene::ComponentId::Camera => edit_camera(ui, world, entity),
        scene::ComponentId::Light => edit_light(ui, world, entity),
    }
}

fn edit_model(
    ui: &mut egui::Ui,
    world: &mut scene::Scene,
    entity: scene::Entity,
    mut mesh: scene::MeshRenderer,
) -> scene::MeshRenderer {
    let scene::Model::Primitive(current) = mesh.model;
    let mut selected = current;
    egui::ComboBox::from_id_salt(("mesh-model", entity))
        .selected_text(primitive_name(selected))
        .show_ui(ui, |ui| {
            for primitive in scene::Primitive::ALL {
                ui.selectable_value(&mut selected, primitive, primitive_name(primitive));
            }
        });
    if selected != current {
        mesh.model = scene::Model::Primitive(selected);
        let _ = world.insert(entity, mesh);
    }
    mesh
}

fn primitive_name(primitive: scene::Primitive) -> &'static str {
    match primitive {
        scene::Primitive::Cube => "Cube",
        scene::Primitive::Sphere => "Sphere",
        scene::Primitive::Cylinder => "Cylinder",
        scene::Primitive::Capsule => "Capsule",
        scene::Primitive::Plane => "Plane",
        scene::Primitive::Cone => "Cone",
        scene::Primitive::Wedge => "Wedge",
    }
}

fn edit_material(
    ui: &mut egui::Ui,
    world: &mut scene::Scene,
    entity: scene::Entity,
    mesh: scene::MeshRenderer,
) {
    let id = egui::Id::new(("mesh-material", entity));
    let mut text = ui
        .data(|data| data.get_temp::<String>(id))
        .unwrap_or_else(|| mesh.material.canonical());
    let response = ui.text_edit_singleline(&mut text);
    if response.changed()
        && let Some(material) = scene::AssetId::parse(&text)
    {
        let _ = world.insert(
            entity,
            scene::MeshRenderer {
                model: mesh.model,
                material,
            },
        );
    }
    if response.lost_focus() {
        ui.data_mut(|data| data.remove::<String>(id));
    } else if response.has_focus() {
        ui.data_mut(|data| data.insert_temp(id, text));
    }
}

fn edit_camera(ui: &mut egui::Ui, world: &mut scene::Scene, entity: scene::Entity) {
    let Some(camera) = world.get::<scene::Camera>(entity).ok().flatten().copied() else {
        return;
    };
    let mut fov = camera.fov_y.to_degrees();
    let mut near = camera.near;
    let mut far = camera.far;
    let clamped = fov < 1.0 || fov > 179.0 || near < 0.01 || far <= near;
    fov = fov.clamp(1.0, 179.0);
    near = near.max(0.01);
    far = far.max(near + 1.0e-3);
    let fov_changed = ui
        .horizontal(|ui| {
            ui.label("Fov");
            ui.add(
                egui::DragValue::new(&mut fov)
                    .range(1.0..=179.0)
                    .suffix("°"),
            )
            .changed()
        })
        .inner;
    let near_changed = ui
        .horizontal(|ui| {
            ui.label("Near");
            ui.add(
                egui::DragValue::new(&mut near)
                    .range(0.01..=1.0e6 - 1.0e-3)
                    .speed(0.01),
            )
            .changed()
        })
        .inner;
    near = near.max(0.01);
    far = far.max(near + 1.0e-3);
    let far_changed = ui
        .horizontal(|ui| {
            ui.label("Far");
            ui.add(
                egui::DragValue::new(&mut far)
                    .speed(0.1)
                    .range(near + 1.0e-3..=1.0e6),
            )
            .changed()
        })
        .inner;
    far = far.max(near + 1.0e-3);
    if clamped || fov_changed || near_changed || far_changed {
        let _ = world.insert(
            entity,
            scene::Camera {
                fov_y: fov.to_radians(),
                near,
                far,
            },
        );
    }
}

fn edit_light(ui: &mut egui::Ui, world: &mut scene::Scene, entity: scene::Entity) {
    let Some(scene::Light::Directional { color, intensity }) =
        world.get::<scene::Light>(entity).ok().flatten().copied()
    else {
        return;
    };
    ui.label("Direction is local -Z");
    let mut rgb = [color.x, color.y, color.z];
    let stored_intensity = intensity;
    let mut intensity = intensity.max(0.0);
    let color_changed = ui.color_edit_button_rgb(&mut rgb).changed();
    let intensity_changed = ui
        .horizontal(|ui| {
            ui.label("Intensity");
            ui.add(
                egui::DragValue::new(&mut intensity)
                    .range(0.0..=1.0e6)
                    .speed(0.05),
            )
            .changed()
        })
        .inner;
    intensity = intensity.max(0.0);
    if color_changed || intensity_changed || intensity != stored_intensity {
        let _ = world.insert(
            entity,
            scene::Light::Directional {
                color: scene::Vec3::new(rgb[0], rgb[1], rgb[2]),
                intensity,
            },
        );
    }
}

fn add_component(
    world: &mut scene::Scene,
    entity: scene::Entity,
    id: scene::ComponentId,
    default_material: scene::AssetId,
) {
    match id {
        scene::ComponentId::Name => {
            let _ = world.insert(entity, scene::Name::new("Entity"));
        }
        scene::ComponentId::Mesh => {
            let _ = world.insert(
                entity,
                scene::MeshRenderer {
                    model: scene::Model::Primitive(scene::Primitive::Cube),
                    material: default_material,
                },
            );
        }
        scene::ComponentId::Camera => {
            let _ = world.insert(
                entity,
                scene::Camera {
                    fov_y: 60.0_f32.to_radians(),
                    near: 0.1,
                    far: 100.0,
                },
            );
        }
        scene::ComponentId::Light => {
            let _ = world.insert(
                entity,
                scene::Light::Directional {
                    color: scene::Vec3::ONE,
                    intensity: 1.0,
                },
            );
        }
    }
}

fn remove_component(world: &mut scene::Scene, entity: scene::Entity, id: scene::ComponentId) {
    match id {
        scene::ComponentId::Name => {
            let _ = world.remove::<scene::Name>(entity);
        }
        scene::ComponentId::Mesh => {
            let _ = world.remove::<scene::MeshRenderer>(entity);
        }
        scene::ComponentId::Camera => {
            let _ = world.remove::<scene::Camera>(entity);
        }
        scene::ComponentId::Light => {
            let _ = world.remove::<scene::Light>(entity);
        }
    }
}

fn component_label(id: scene::ComponentId) -> &'static str {
    match id {
        scene::ComponentId::Name => "Name",
        scene::ComponentId::Mesh => "Mesh",
        scene::ComponentId::Camera => "Camera",
        scene::ComponentId::Light => "Light",
    }
}

fn drag_vec3(ui: &mut egui::Ui, label: &str, value: &mut scene::Vec3) -> bool {
    ui.horizontal(|ui| {
        ui.label(label);
        let x = ui
            .add(
                egui::DragValue::new(&mut value.x)
                    .speed(0.05)
                    .fixed_decimals(2),
            )
            .changed();
        let y = ui
            .add(
                egui::DragValue::new(&mut value.y)
                    .speed(0.05)
                    .fixed_decimals(2),
            )
            .changed();
        let z = ui
            .add(
                egui::DragValue::new(&mut value.z)
                    .speed(0.05)
                    .fixed_decimals(2),
            )
            .changed();
        x || y || z
    })
    .inner
}

fn drag_euler(ui: &mut egui::Ui, degrees: &mut [f32; 3]) -> bool {
    ui.horizontal(|ui| {
        ui.label("Rotation");
        let yaw = ui
            .add(
                egui::DragValue::new(&mut degrees[0])
                    .speed(1.0)
                    .suffix("°")
                    .fixed_decimals(1),
            )
            .changed();
        let pitch = ui
            .add(
                egui::DragValue::new(&mut degrees[1])
                    .speed(1.0)
                    .suffix("°")
                    .fixed_decimals(1),
            )
            .changed();
        let roll = ui
            .add(
                egui::DragValue::new(&mut degrees[2])
                    .speed(1.0)
                    .suffix("°")
                    .fixed_decimals(1),
            )
            .changed();
        yaw || pitch || roll
    })
    .inner
}
