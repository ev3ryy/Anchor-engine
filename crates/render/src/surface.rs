use ash::{Entry, Instance, khr::surface, vk};
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

use crate::Error;

pub struct SurfaceBundle {
    pub loader: surface::Instance,
    pub surface: vk::SurfaceKHR,
}

impl Drop for SurfaceBundle {
    fn drop(&mut self) {
        unsafe {
            self.loader.destroy_surface(self.surface, None);
        }
    }
}

pub fn create(
    entry: &Entry,
    instance: &Instance,
    window: &(impl HasDisplayHandle + HasWindowHandle),
) -> Result<SurfaceBundle, Error> {
    let display_handle = window
        .display_handle()
        .map_err(Error::WindowHandle)?
        .as_raw();
    let window_handle = window
        .window_handle()
        .map_err(Error::WindowHandle)?
        .as_raw();
    let surface = unsafe {
        ash_window::create_surface(entry, instance, display_handle, window_handle, None)?
    };
    let loader = surface::Instance::new(entry, instance);
    Ok(SurfaceBundle { loader, surface })
}
