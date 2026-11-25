pub use winit;

use winit::error::OsError;
use winit::event_loop::ActiveEventLoop;
use winit::window::Window;

pub fn create(event_loop: &ActiveEventLoop) -> Result<Window, OsError> {
    let attrs = Window::default_attributes().with_title("Anchor Engine");
    event_loop.create_window(attrs)
}
