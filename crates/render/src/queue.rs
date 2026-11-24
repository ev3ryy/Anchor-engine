use ash::{Device, Instance, khr::surface, vk};

use crate::Error;

pub struct Queue {
    pub family: u32,
    pub handle: vk::Queue,
}

pub fn get(device: &Device, family: u32) -> Queue {
    Queue {
        family,
        handle: unsafe { device.get_device_queue(family, 0) },
    }
}

pub fn find_graphics_present_family(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    surface_loader: &surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<u32, Error> {
    let families = unsafe { instance.get_physical_device_queue_family_properties(physical_device) };
    let mut found_graphics = false;

    for (index, props) in families.iter().enumerate() {
        if !props.queue_flags.contains(vk::QueueFlags::GRAPHICS) {
            continue;
        }
        found_graphics = true;

        let supports_present = unsafe {
            surface_loader.get_physical_device_surface_support(
                physical_device,
                index as u32,
                surface,
            )?
        };
        if supports_present {
            return Ok(index as u32);
        }
    }

    if found_graphics {
        Err(Error::NoPresentQueue)
    } else {
        Err(Error::NoGraphicsQueue)
    }
}
