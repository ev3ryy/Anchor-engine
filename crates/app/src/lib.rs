mod map;

use window::winit::application::ApplicationHandler;
use window::winit::event::{DeviceEvent, DeviceId, WindowEvent};
use window::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use window::winit::keyboard::PhysicalKey;
use window::winit::window::{Window, WindowId};

pub struct App {
    render: Option<render::Context>,
    window: Option<Window>,
    input: input::Input,
    camera: render::Camera,
    material: Option<render::MaterialId>,
    last_frame: Option<std::time::Instant>,
}

impl App {
    fn request_redraw(&self) {
        if let Some(window) = self.window.as_ref() {
            window.request_redraw();
        }
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

        window.request_redraw();
        self.material = Some(material);
        self.render = Some(render);
        self.window = Some(window);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                let Some(render) = self.render.as_mut() else {
                    return;
                };
                if let Err(err) = render.resize(size.width, size.height) {
                    eprintln!("failed to resize swapchain: {err}");
                    event_loop.exit();
                    return;
                }
                if size.width > 0 && size.height > 0 {
                    if let Some(window) = self.window.as_ref() {
                        window.request_redraw();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let Some(material) = self.material else {
                    return;
                };
                let Some(render) = self.render.as_mut() else {
                    return;
                };
                let Some(window) = self.window.as_ref() else {
                    return;
                };
                let now = std::time::Instant::now();
                let dt = self
                    .last_frame
                    .map(|then| (now - then).as_secs_f32())
                    .unwrap_or(0.0)
                    .min(0.1);
                self.last_frame = Some(now);
                drive_camera(&mut self.camera, &self.input, dt);
                let mut failed = false;
                match render.draw(&self.camera, material) {
                    Ok(render::DrawStatus::Ok) => {}
                    Ok(render::DrawStatus::OutOfDate) => {
                        let size = window.inner_size();
                        if let Err(err) = render.resize(size.width, size.height) {
                            eprintln!("failed to resize swapchain: {err}");
                            failed = true;
                        } else if size.width > 0 && size.height > 0 {
                            window.request_redraw();
                        }
                    }
                    Err(err) => {
                        eprintln!("failed to draw: {err}");
                        failed = true;
                    }
                }
                self.input.end_frame();
                if failed {
                    event_loop.exit();
                    return;
                }
                if self.input.any_down() {
                    let size = window.inner_size();
                    if size.width > 0 && size.height > 0 {
                        window.request_redraw();
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
                self.input.set_mouse_button(button, state.is_pressed());
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
                self.input.add_scroll(x, y);
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
        render: None,
        window: None,
        input: input::Input::default(),
        camera: render::Camera::new(),
        material: None,
        last_frame: None,
    })
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

fn drive_camera(camera: &mut render::Camera, input: &input::Input, dt: f32) {
    let mut wish = [0.0; 3];
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
    if input.key(input::Key::S) {
        wish[2] -= 1.0;
    }

    let look = if input.mouse_button(input::MouseButton::Right) {
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
