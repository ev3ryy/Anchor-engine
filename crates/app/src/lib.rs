use window::winit::application::ApplicationHandler;
use window::winit::event::WindowEvent;
use window::winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use window::winit::window::{Window, WindowId};

pub struct App {
    render: Option<render::Context>,
    window: Option<Window>,
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
        let render = match render::Context::new(&window, size.width, size.height) {
            Ok(render) => render,
            Err(err) => {
                eprintln!("failed to initialize Vulkan: {err}");
                event_loop.exit();
                return;
            }
        };

        window.request_redraw();
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
                let Some(render) = self.render.as_mut() else {
                    return;
                };
                let Some(window) = self.window.as_ref() else {
                    return;
                };
                match render.draw() {
                    Ok(render::DrawStatus::Ok) => {}
                    Ok(render::DrawStatus::OutOfDate) => {
                        let size = window.inner_size();
                        if let Err(err) = render.resize(size.width, size.height) {
                            eprintln!("failed to resize swapchain: {err}");
                            event_loop.exit();
                            return;
                        }
                        if size.width > 0 && size.height > 0 {
                            window.request_redraw();
                        }
                    }
                    Err(err) => {
                        eprintln!("failed to draw: {err}");
                        event_loop.exit();
                    }
                }
            }
            _ => {}
        }
    }
}

pub fn run() -> Result<(), window::winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    event_loop.run_app(&mut App {
        render: None,
        window: None,
    })
}
