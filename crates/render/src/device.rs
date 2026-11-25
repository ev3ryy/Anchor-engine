use ash::{Device, Instance, khr::surface, vk};

use crate::{Error, queue, supports_vulkan_1_3};

pub struct DeviceBundle {
    pub physical_device: vk::PhysicalDevice,
    pub device: Device,
    pub graphics: queue::Queue,
    pub texture_capacity: u32,
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
    let (physical_device, texture_capacity) =
        pick_physical_device(instance, surface_loader, surface)?;
    let family =
        queue::find_graphics_present_family(instance, physical_device, surface_loader, surface)?;

    let queue_priorities = [1.0_f32];
    let queue_info = vk::DeviceQueueCreateInfo::default()
        .queue_family_index(family)
        .queue_priorities(&queue_priorities);

    let mut vulkan12 = vk::PhysicalDeviceVulkan12Features::default()
        .descriptor_indexing(true)
        .runtime_descriptor_array(true)
        .descriptor_binding_partially_bound(true)
        .descriptor_binding_sampled_image_update_after_bind(true)
        .descriptor_binding_update_unused_while_pending(true)
        .shader_sampled_image_array_non_uniform_indexing(true)
        .scalar_block_layout(true)
        .buffer_device_address(true);
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
        .push_next(&mut vulkan12)
        .push_next(&mut vulkan13)
        .push_next(&mut maintenance5);

    let device = unsafe { instance.create_device(physical_device, &create_info, None) }?;
    let graphics = queue::get(&device, family);

    Ok(DeviceBundle {
        physical_device,
        device,
        graphics,
        texture_capacity,
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

pub(crate) const MIN_TEXTURE_CAPACITY: u32 = 64;
const MAX_TEXTURE_CAPACITY: u32 = 1024;

pub(crate) fn texture_capacity(
    per_stage_images: u32,
    per_set_images: u32,
    per_stage_resources: u32,
) -> u32 {
    per_stage_images
        .min(per_set_images)
        .min(per_stage_resources)
        .min(MAX_TEXTURE_CAPACITY)
}

fn bindless_capacity(instance: &Instance, device: vk::PhysicalDevice) -> Option<u32> {
    let mut features12 = vk::PhysicalDeviceVulkan12Features::default();
    let mut features = vk::PhysicalDeviceFeatures2::default().push_next(&mut features12);
    unsafe { instance.get_physical_device_features2(device, &mut features) };
    if features12.descriptor_indexing == 0
        || features12.runtime_descriptor_array == 0
        || features12.descriptor_binding_partially_bound == 0
        || features12.descriptor_binding_sampled_image_update_after_bind == 0
        || features12.descriptor_binding_update_unused_while_pending == 0
        || features12.shader_sampled_image_array_non_uniform_indexing == 0
        || features12.scalar_block_layout == 0
        || features12.buffer_device_address == 0
    {
        return None;
    }

    let mut properties12 = vk::PhysicalDeviceVulkan12Properties::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut properties12);
    unsafe { instance.get_physical_device_properties2(device, &mut properties) };
    let capacity = texture_capacity(
        properties12.max_per_stage_descriptor_update_after_bind_sampled_images,
        properties12.max_descriptor_set_update_after_bind_sampled_images,
        properties12.max_per_stage_update_after_bind_resources,
    );
    (capacity >= MIN_TEXTURE_CAPACITY).then_some(capacity)
}

fn pick_physical_device(
    instance: &Instance,
    surface_loader: &surface::Instance,
    surface: vk::SurfaceKHR,
) -> Result<(vk::PhysicalDevice, u32), Error> {
    let devices = unsafe { instance.enumerate_physical_devices() }?;
    let mut best: Option<(u8, vk::PhysicalDevice, u32)> = None;
    let mut present_unsupported = false;
    let mut maintenance5_missing = false;
    let mut bindless_missing = false;

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

        let Some(capacity) = bindless_capacity(instance, device) else {
            bindless_missing = true;
            continue;
        };

        let rank = device_type_rank(props.device_type);
        if best.is_none_or(|(best_rank, _, _)| rank < best_rank) {
            best = Some((rank, device, capacity));
        }
    }

    let Some((_, physical, capacity)) = best else {
        return Err(if present_unsupported {
            Error::NoPresentQueue
        } else if bindless_missing {
            Error::NoBindless
        } else if maintenance5_missing {
            Error::NoMaintenance5
        } else {
            Error::NoPhysicalDevice
        });
    };

    let props = unsafe { instance.get_physical_device_properties(physical) };
    match props.device_name_as_c_str() {
        Ok(name) => eprintln!("Using GPU: {name:?} ({capacity} bindless textures)"),
        Err(_) => eprintln!("Using GPU: <invalid name> ({capacity} bindless textures)"),
    }

    Ok((physical, capacity))
}

#[cfg(test)]
mod tests {
    use super::{MAX_TEXTURE_CAPACITY, MIN_TEXTURE_CAPACITY, texture_capacity};

    #[test]
    fn texture_capacity_uses_both_image_limits_and_the_stage_budget() {
        assert_eq!(texture_capacity(4096, 8192, 2048), MAX_TEXTURE_CAPACITY);
        assert_eq!(texture_capacity(512, 256, 1024), 256);
        assert_eq!(texture_capacity(128, 4096, 64), 64);
        assert!(texture_capacity(32, 4096, 4096) < MIN_TEXTURE_CAPACITY);
    }
}
