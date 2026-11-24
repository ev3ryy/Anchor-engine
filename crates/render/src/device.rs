use ash::{Device, Instance, khr::surface, vk};

use crate::{Error, queue, supports_vulkan_1_3};

pub struct DeviceBundle {
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub graphics: queue::Queue,
}

impl Drop for DeviceBundle {
    fn drop(&mut self) {
        unsafe {
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
    let family =
        queue::find_graphics_present_family(instance, physical_device, surface_loader, surface)?;

    let queue_priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&queue_priorities);

    let mut vulkan13 = vk::PhysicalDeviceVulkan13Features::default()
        .synchronization2(true)
        .dynamic_rendering(true);
    let mut maintenance5 = vk::PhysicalDeviceMaintenance5FeaturesKHR::default().maintenance5(true);
    let extension_names = [
        ash::khr::swapchain::NAME.as_ptr(),
        ash::khr::maintenance5::NAME.as_ptr(),
    ];
    let create_info = vk::DeviceCreateInfo::default()
        .queue_create_infos(std::slice::from_ref(&queue_info))
        .enabled_extension_names(&extension_names)
        .push_next(&mut vulkan13)
        .push_next(&mut maintenance5);

    let device = unsafe { instance.create_device(physical_device, &create_info, None) }?;
    let graphics = queue::get(&device, family);

    Ok(DeviceBundle {
        physical_device,
        device,
        graphics,
    })
}

fn has_maintenance5(instance: &Instance, device: vk::PhysicalDevice) -> Result<bool, Error> {
    let extensions = unsafe { instance.enumerate_device_extension_properties(device)? };
    Ok(extensions.iter().any(|extension| {
        extension
            .extension_name_as_c_str()
            .is_ok_and(|name| name == ash::khr::maintenance5::NAME)
    }))
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
    let mut maintenance5_missing = false;

    for &device in &devices {
        let props = unsafe { instance.get_physical_device_properties(device) };
        if !supports_vulkan_1_3(props.api_version) {
            continue;
        }
        match has_maintenance5(instance, device) {
            Ok(true) => {}
            Ok(false) => {
                maintenance5_missing = true;
                continue;
            }
            Err(err) => return Err(err),
        }

        match queue::find_graphics_present_family(instance, device, surface_loader, surface) {
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
        } else if maintenance5_missing {
            Error::NoMaintenance5
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
