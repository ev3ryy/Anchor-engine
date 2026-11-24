use ash::{Device, Instance, khr::surface, vk};

use crate::{Error, supports_vulkan_1_3};

pub struct DeviceBundle {
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub graphics_queue_family: u32,
    pub graphics_queue: vk::Queue,
}

impl Drop for DeviceBundle {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device_wait_idle();
            self.device.destroy_device(None);
        }
    }
}

pub fn create(
    instance: &Instance,
    surface_loader: &surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<DeviceBundle, Error> {
    let physical_device = pick_physical_device(instance, surface_loader, surface)?;
    let graphics_queue_family =
        find_graphics_present_queue_family(instance, physical_device, surface_loader, surface)?;

    let queue_priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(graphics_queue_family)
        .queue_priorities(&queue_priorities);

    let extension_names = [ash::khr::swapchain::NAME.as_ptr()];
    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_info))
        .enabled_extension_names(&extension_names);

    let device = unsafe { instance.create_device(physical_device, &create_info, None) }?;
    let graphics_queue = unsafe { device.get_device_queue(graphics_queue_family, 0) };

    Ok(DeviceBundle {
        physical_device,
        device,
        graphics_queue_family,
        graphics_queue,
    })
}

fn device_type_rank(device_type: vk::PhysicalDeviceType) -> u8 {
    match device_type {
        vk::PhysicalDeviceType::DISCRETE_GPU => 0,
        vk::PhysicalDeviceType::INTEGRATED_GPU => 1,
        vk::PhysicalDeviceType::VIRTUAL_GPU => 2,
        vk::PhysicalDeviceType::CPU => 3,
        _ => 4,
    }
}

fn pick_physical_device(
    instance: &Instance,
    surface_loader: &surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<vk::PhysicalDevice, Error> {
    let devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut best: Option<(u8, vk::PhysicalDevice)> = None;
    let mut present_unsupported = false;

    for &device in &devices {
        let props = unsafe { instance.get_physical_device_properties(device) };
        if !supports_vulkan_1_3(props.api_version) {
            continue;
        }

        match find_graphics_present_queue_family(instance, device, surface_loader, surface) {
            Ok(_) => {}
            Err(Error::NoGraphicsQueue) => continue,
            Err(Error::NoPresentQueue) => {
                present_unsupported = true;
                continue;
            }
            Err(err) => return Err(err),
        }

        let rank = device_type_rank(props.device_type);
        if best.is_none_or(|(best_rank, _)| rank < best_rank) {
            best = Some((rank, device));
        }
    }

    let Some((_, physical)) = best else {
        return Err(if present_unsupported {
            Error::NoPresentQueue
        } else {
            Error::NoPhysicalDevice
        });
    };

    let props = unsafe { instance.get_physical_device_properties(physical) };
    match props.device_name_as_c_str() {
        Ok(name) => eprintln!("Using GPU: {name:?}"),
        Err(_) => eprintln!("Using GPU: <invalid name>"),
    }

    Ok(physical)
}

fn find_graphics_present_queue_family(
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
