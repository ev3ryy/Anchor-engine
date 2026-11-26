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
    pub world: &'a scene::Scene,
    pub camera: &'a Camera,
    pub dt: f32,
    pub extent: [u32; 2],
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
                if self.world.is_empty() {
                    ui.label("No scene");
                } else {
                    for root in self.world.roots() {
                        hierarchy(ui, self.world, root, 0);
                    }
                }
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
            }
            Tab::Inspector => {
                let position = self.camera.position();
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
            }
            Tab::Stats => {
                ui.label(format!("frame     {:>7.2} ms", self.dt * 1000.0));
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

fn hierarchy(ui: &mut egui::Ui, world: &scene::Scene, entity: scene::Entity, depth: usize) {
    ui.horizontal(|ui| {
        ui.add_space(depth as f32 * 12.0);
        let label = world.name(entity).ok().flatten().unwrap_or("Entity");
        ui.label(label);
    });
    if let Ok(children) = world.children(entity) {
        for child in children {
            hierarchy(ui, world, child, depth + 1);
        }
    }
}
