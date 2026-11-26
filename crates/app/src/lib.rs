mod map;

use std::path::{Path, PathBuf};

use window::winit::application::ApplicationHandler;
use window::winit::event::{DeviceEvent, DeviceId, WindowEvent};
use window::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use window::winit::keyboard::PhysicalKey;
use window::winit::window::{Window, WindowId};

const CHECKER: scene::AssetId = scene::AssetId([
    0xc0, 0xff, 0xee, 0x00, 0x00, 0x00, 0x40, 0x00, 0x80, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
]);

const SCENE_GUID: &str = "c0ffee00-0000-4000-8000-000000000002";

pub struct App {
    editor: Option<editor::Editor>,
    render: Option<render::Context>,
    window: Option<Window>,
    input: input::Input,
    camera: render::Camera,
    scene: scene::Scene,
    material: Option<scene::MaterialRef>,
    extract: scene::Extract,
    last_frame: Option<std::time::Instant>,
    looking: bool,
    scene_path: Option<PathBuf>,
    scene_guid: String,
    file_status: String,
}

impl App {
    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
    }

    fn resize_surface(&mut self, width: u32, height: u32) -> Result<(), ()> {
        let format = {
            let Some(render) = self.render.as_mut() else {
                return Ok(());
            };
            if let Err(err) = render.resize(width, height) {
                eprintln!("failed to resize swapchain: {err}");
                return Err(());
            }
            render.color_format()
        };
        if let Some(editor) = self.editor.as_mut()
            && let Err(err) = editor.sync_format(format)
        {
            eprintln!("failed to resize editor: {err}");
            return Err(());
        }
        Ok(())
    }

    fn apply_file_actions(&mut self, actions: Vec<editor::FileAction>) {
        for action in actions {
            match action {
                editor::FileAction::Open => self.open_scene(),
                editor::FileAction::Save => self.save_scene(false),
                editor::FileAction::SaveAs => self.save_scene(true),
            }
        }
    }

    fn save_scene(&mut self, save_as: bool) {
        let Some(path) = self.pick_save_path(save_as) else {
            return;
        };
        match write_scene(&self.scene, &self.scene_guid, &path) {
            Ok(()) => {
                self.file_status = format!("Saved {}", path.display());
                self.scene_path = Some(path);
            }
            Err(err) => {
                eprintln!("failed to save scene: {err}");
                self.file_status = format!("Save failed: {err}");
            }
        }
    }

    fn open_scene(&mut self) {
        let Some(path) = self.pick_open_path() else {
            return;
        };
        match read_scene(&path) {
            Ok((scene, guid)) => {
                self.scene = scene;
                self.scene_guid = guid;
                self.scene_path = Some(path.clone());
                if let Some(editor) = self.editor.as_mut() {
                    editor.clear_selection();
                }
                self.scene.update();
                self.file_status = format!("Opened {}", path.display());
            }
            Err(err) => {
                eprintln!("failed to open scene: {err}");
                self.file_status = format!("Open failed: {err}");
            }
        }
    }

    fn pick_save_path(&self, save_as: bool) -> Option<PathBuf> {
        if !save_as && let Some(path) = self.scene_path.clone() {
            return Some(path);
        }
        file_dialog(self.window.as_ref())
            .save_file()
            .map(ensure_ron)
    }

    fn pick_open_path(&self) -> Option<PathBuf> {
        file_dialog(self.window.as_ref()).pick_file()
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let window = match window::create(event_loop) {
            Ok(window) => window,
            Err(err) => {
                eprintln!("failed to create window: {err}");
                event_loop.exit();
                return;
            }
        };
        let size = window.inner_size();
        let mut render = match render::Context::new(&window, size.width, size.height) {
            Ok(render) => render,
            Err(err) => {
                eprintln!("failed to initialize Vulkan: {err}");
                event_loop.exit();
                return;
            }
        };
        let pixels = checker(64);
        let texture = match render.create_texture(64, 64, &pixels) {
            Ok(texture) => texture,
            Err(err) => {
                eprintln!("failed to create texture: {err}");
                event_loop.exit();
                return;
            }
        };
        let material = match render.create_material(render::MaterialDesc::Unlit {
            color: [1.0, 0.92, 0.75, 1.0],
            albedo: texture,
            sampler: render::SamplerId::LinearRepeat,
        }) {
            Ok(material) => material,
            Err(err) => {
                eprintln!("failed to create material: {err}");
                event_loop.exit();
                return;
            }
        };

        let editor = match editor::Editor::new(&mut render, &window) {
            Ok(editor) => editor,
            Err(err) => {
                eprintln!("failed to create editor: {err}");
                event_loop.exit();
                return;
            }
        };

        let material = scene::MaterialRef {
            index: material.index,
            generation: material.generation,
        };
        attach_child_mesh(&mut self.scene, CHECKER);
        self.material = Some(material);
        window.request_redraw();
        self.editor = Some(editor);
        self.render = Some(render);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        let egui_repaint = {
            let App { editor, window, .. } = self;
            match (editor.as_mut(), window.as_ref()) {
                (Some(editor), Some(window)) => editor.on_window_event(window, &event),
                _ => false,
            }
        };
        if egui_repaint {
            self.request_redraw();
        }

        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if self.resize_surface(size.width, size.height).is_err() {
                    event_loop.exit();
                    return;
                }
                if size.width > 0 && size.height > 0 {
                    self.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => {
                if self.window.is_none() {
                    return;
                }
                let now = std::time::Instant::now();
                let dt = self
                    .last_frame
                    .map(|then| (now - then).as_secs_f32())
                    .unwrap_or(0.0)
                    .min(0.1);
                self.last_frame = Some(now);
                let keyboard_captured = self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.wants_keyboard());
                drive_camera(
                    &mut self.camera,
                    &self.input,
                    dt,
                    self.looking,
                    keyboard_captured,
                );
                let extent = self
                    .window
                    .as_ref()
                    .map(|window| window.inner_size())
                    .unwrap_or_default();
                let actions = {
                    let App {
                        editor,
                        camera,
                        scene,
                        material,
                        window,
                        file_status,
                        ..
                    } = self;
                    if let (Some(editor), Some(window), Some(_)) =
                        (editor.as_mut(), window.as_ref(), material.as_ref())
                    {
                        editor.begin_frame(
                            window,
                            editor::FrameView {
                                camera,
                                scene,
                                default_material: CHECKER,
                                dt,
                                extent: [extent.width, extent.height],
                                status: file_status,
                            },
                        )
                    } else {
                        Vec::new()
                    }
                };
                if !actions.is_empty() {
                    self.apply_file_actions(actions);
                    self.request_redraw();
                }
                self.scene.update();
                let gpu_material = self.material;
                self.scene.extract_into(&mut self.extract, |asset| {
                    if asset == CHECKER { gpu_material } else { None }
                });
                let target = self
                    .editor
                    .as_ref()
                    .and_then(|editor| editor.viewport_pixels())
                    .map(render::RenderTarget::Viewport)
                    .unwrap_or(render::RenderTarget::Viewport([1, 1]));
                let editor = std::cell::RefCell::new(self.editor.take());
                let draw = {
                    let Some(render) = self.render.as_mut() else {
                        self.editor = editor.into_inner();
                        return;
                    };
                    render.draw(
                        &self.camera,
                        &self.extract.draws,
                        target,
                        |queue| {
                            editor
                                .borrow_mut()
                                .as_mut()
                                .map(|editor| editor.prepare(queue))
                                .unwrap_or(Ok(()))
                        },
                        |overlay| {
                            editor
                                .borrow_mut()
                                .as_mut()
                                .map(|editor| editor.paint(overlay))
                                .unwrap_or(Ok(()))
                        },
                    )
                };
                self.editor = editor.into_inner();
                let mut failed = false;
                match draw {
                    Ok(render::DrawStatus::Ok) => {}
                    Ok(render::DrawStatus::OutOfDate) => {
                        let size = self
                            .window
                            .as_ref()
                            .map(|window| window.inner_size())
                            .unwrap_or_default();
                        if self.resize_surface(size.width, size.height).is_err() {
                            failed = true;
                        } else if size.width > 0 && size.height > 0 {
                            self.request_redraw();
                        }
                    }
                    Err(err) => {
                        eprintln!("failed to draw: {err}");
                        failed = true;
                    }
                }
                let ctrl = self.input.key(input::Key::ControlLeft)
                    || self.input.key(input::Key::ControlRight);
                let save_shortcut = ctrl
                    && self.input.key_pressed(input::Key::S)
                    && !self
                        .editor
                        .as_ref()
                        .is_some_and(|editor| editor.wants_keyboard());
                if save_shortcut {
                    self.save_scene(false);
                    self.request_redraw();
                }
                self.input.end_frame();
                if failed {
                    event_loop.exit();
                    return;
                }
                let repaint = self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.wants_repaint());
                if repaint || self.input.any_down() || self.looking {
                    let size = self
                        .window
                        .as_ref()
                        .map(|window| window.inner_size())
                        .unwrap_or_default();
                    if size.width > 0 && size.height > 0 {
                        self.request_redraw();
                    }
                }
            }
            WindowEvent::KeyboardInput {
                event,
                is_synthetic,
                ..
            } => {
                if is_synthetic && event.state.is_pressed() {
                    return;
                }
                let PhysicalKey::Code(code) = event.physical_key else {
                    return;
                };
                let Some(key) = map::key(code) else {
                    return;
                };
                self.input
                    .set_key(key, event.state.is_pressed(), event.repeat);
                self.request_redraw();
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let Some(button) = map::mouse_button(button) else {
                    return;
                };
                let pressed = state.is_pressed();
                if button == input::MouseButton::Right {
                    if pressed {
                        let cursor = self.input.cursor();
                        self.looking = self.editor.as_ref().is_some_and(|editor| {
                            editor.pointer_in_viewport(cursor) && !editor.egui_is_using_pointer()
                        });
                    } else {
                        self.looking = false;
                    }
                }
                self.input.set_mouse_button(button, pressed);
                self.request_redraw();
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.input.set_cursor(position.x as f32, position.y as f32);
                self.request_redraw();
            }
            WindowEvent::CursorEntered { .. } => {
                self.input.set_cursor_inside(true);
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                self.input.set_cursor_inside(false);
                self.request_redraw();
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (x, y) = map::scroll(delta);
                let cursor = self.input.cursor();
                if self
                    .editor
                    .as_ref()
                    .is_some_and(|editor| editor.pointer_in_viewport(cursor))
                {
                    self.input.add_scroll(x, y);
                }
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.input.set_focused(focused);
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn device_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _device_id: DeviceId,
        event: DeviceEvent,
    ) {
        let DeviceEvent::MouseMotion { delta } = event else {
            return;
        };
        if self.input.focused() && self.input.cursor_inside() {
            self.input.add_mouse_delta(delta.0 as f32, delta.1 as f32);
            self.request_redraw();
        }
    }
}

pub fn run() -> Result<(), window::winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App {
        editor: None,
        render: None,
        window: None,
        input: input::Input::default(),
        camera: render::Camera::new(),
        scene: demo_scene(),
        material: None,
        extract: scene::Extract::default(),
        last_frame: None,
        looking: false,
        scene_path: None,
        scene_guid: SCENE_GUID.to_string(),
        file_status: String::new(),
    })
}

fn file_dialog(window: Option<&Window>) -> rfd::FileDialog {
    let dialog = rfd::FileDialog::new().add_filter("Scene", &["ron"]);
    match window {
        Some(window) => dialog.set_parent(window),
        None => dialog,
    }
}

fn ensure_ron(path: PathBuf) -> PathBuf {
    if path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("ron"))
    {
        path
    } else {
        path.with_extension("ron")
    }
}

fn write_scene(scene: &scene::Scene, guid: &str, path: &Path) -> Result<(), String> {
    let document = asset::scene_to_document(scene, guid, &asset::Assets::default())
        .map_err(|err| err.to_string())?;
    let text = asset::format_document(&document).map_err(|err| err.to_string())?;
    std::fs::write(path, text).map_err(|err| err.to_string())
}

fn read_scene(path: &Path) -> Result<(scene::Scene, String), String> {
    let text = std::fs::read_to_string(path).map_err(|err| err.to_string())?;
    let document = asset::parse_document(&text).map_err(|err| err.to_string())?;
    if scene::AssetId::parse(&document.guid).is_none() {
        return Err(format!("asset guid: {}", document.guid));
    }
    let guid = document.guid.clone();
    let scene = asset::document_to_scene(&document, &asset::Assets::default())
        .map_err(|err| err.to_string())?;
    Ok((scene, guid))
}

fn demo_scene() -> scene::Scene {
    let mut scene = scene::Scene::new();
    let root = scene.spawn();
    scene.insert(root, scene::Name::new("Root")).expect("root");
    let child = scene.spawn_child(root).expect("root");
    scene
        .insert(child, scene::Name::new("Child"))
        .expect("child");
    scene
        .set_local_position(child, scene::Vec3::X)
        .expect("child");
    scene.update();
    scene
}

fn attach_child_mesh(scene: &mut scene::Scene, material: scene::AssetId) {
    let roots: Vec<_> = scene.roots().collect();
    let mut children = Vec::new();
    for root in roots {
        if let Ok(kids) = scene.children(root) {
            children.extend(kids);
        }
    }
    for child in children {
        if scene
            .get::<scene::Name>(child)
            .ok()
            .flatten()
            .is_some_and(|name| name == "Child")
        {
            scene
                .insert(
                    child,
                    scene::MeshRenderer {
                        mesh: scene::Mesh::Cube,
                        material,
                    },
                )
                .expect("child mesh");
        }
    }
}

fn checker(size: u32) -> Vec<u8> {
    let mut pixels = Vec::with_capacity((size * size * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let on = (x / 8 + y / 8).is_multiple_of(2);
            let (red, green, blue) = if on { (230, 120, 40) } else { (25, 32, 48) };
            pixels.extend_from_slice(&[red, green, blue, 255]);
        }
    }
    pixels
}

fn drive_camera(
    camera: &mut render::Camera,
    input: &input::Input,
    dt: f32,
    looking: bool,
    keyboard_captured: bool,
) {
    let mut wish = [0.0; 3];
    if !keyboard_captured {
        if input.key(input::Key::D) {
            wish[0] += 1.0;
        }
        if input.key(input::Key::A) {
            wish[0] -= 1.0;
        }
        if input.key(input::Key::E) {
            wish[1] += 1.0;
        }
        if input.key(input::Key::Q) {
            wish[1] -= 1.0;
        }
        if input.key(input::Key::W) {
            wish[2] += 1.0;
        }
        let ctrl = input.key(input::Key::ControlLeft) || input.key(input::Key::ControlRight);
        if input.key(input::Key::S) && !ctrl {
            wish[2] -= 1.0;
        }
    }

    let look = if looking {
        input.mouse_delta()
    } else {
        [0.0, 0.0]
    };
    let boost = if input.key(input::Key::ShiftLeft) || input.key(input::Key::ShiftRight) {
        4.0
    } else {
        1.0
    };
    camera.scroll_speed(input.scroll()[1]);
    camera.fly(wish, look, dt, boost);
}
