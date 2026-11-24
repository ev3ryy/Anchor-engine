use ash::{Device, Instance, khr::swapchain, vk};

use crate::Error;

pub struct SwapchainBundle {
    pub loader: swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    #[allow(dead_code)]
    pub images: Vec<vk::Image>,
    #[allow(dead_code)]
    pub format: vk::Format,
    #[allow(dead_code)]
    pub extent: vk::Extent2D,
}

impl Drop for SwapchainBundle {
    fn drop(&mut self) {
        if self.swapchain != vk::SwapchainKHR::null() {
            unsafe {
                self.loader.destroy_swapchain(self.swapchain, None);
            }
        }
    }
}

pub fn create(
    instance: &Instance,
    device: &Device,
    physical_device: vk::PhysicalDevice,
    surface_loader: &ash::khr::surface::Instance,
    surface: vk::SurfaceKHR,
    width: u32,
    height: u32,
    old_swapchain: vk::SwapchainKHR,
) -> Result<SwapchainBundle, Error> {
    let capabilities = unsafe {
        surface_loader.get_physical_device_surface_capabilities(physical_device, surface)?
    };
    let formats =
        unsafe { surface_loader.get_physical_device_surface_formats(physical_device, surface)? };
    let present_modes = unsafe {
        surface_loader.get_physical_device_surface_present_modes(physical_device, surface)?
    };

    let (format, color_space) = choose_format(&formats).ok_or(Error::NoSurfaceFormat)?;
    let present_mode = choose_present_mode(&present_modes);
    let extent = choose_extent(&capabilities, width, height);
    let composite_alpha = choose_composite_alpha(capabilities.supported_composite_alpha);

    let mut image_count = capabilities.min_image_count + 1;
    if capabilities.max_image_count > 0 {
        image_count = image_count.min(capabilities.max_image_count);
    }

    let create_info = vk::SwapchainCreateInfoKHR::default()
        .surface(surface)
        .min_image_count(image_count)
        .image_format(format)
        .image_color_space(color_space)
        .image_extent(extent)
        .image_array_layers(1)
        .image_usage(vk::ImageUsageFlags::COLOR_ATTACHMENT)
        .image_sharing_mode(vk::SharingMode::EXCLUSIVE)
        .pre_transform(capabilities.current_transform)
        .composite_alpha(composite_alpha)
        .present_mode(present_mode)
        .clipped(true)
        .old_swapchain(old_swapchain);

    let loader = swapchain::Device::new(instance, device);
    let swapchain = unsafe { loader.create_swapchain(&create_info, None)? };
    let images = unsafe { loader.get_swapchain_images(swapchain)? };

    Ok(SwapchainBundle {
        loader,
        swapchain,
        images,
        format,
        extent,
    })
}

fn choose_format(formats: &[vk::SurfaceFormatKHR]) -> Option<(vk::Format, vk::ColorSpaceKHR)> {
    const PREFERRED: &[vk::Format] = &[vk::Format::B8G8R8A8_SRGB, vk::Format::R8G8B8A8_SRGB];

    for &preferred in PREFERRED {
        if let Some(format) = formats.iter().find(|format| {
            format.format == preferred && format.color_space == vk::ColorSpaceKHR::SRGB_NONLINEAR
        }) {
            return Some((format.format, format.color_space));
        }
    }

    formats
        .first()
        .map(|format| (format.format, format.color_space))
}

fn choose_present_mode(modes: &[vk::PresentModeKHR]) -> vk::PresentModeKHR {
    if modes.contains(&vk::PresentModeKHR::MAILBOX) {
        vk::PresentModeKHR::MAILBOX
    } else {
        vk::PresentModeKHR::FIFO
    }
}

fn choose_extent(
    capabilities: &vk::SurfaceCapabilitiesKHR,
    width: u32,
    height: u32,
) -> vk::Extent2D {
    if capabilities.current_extent.width != u32::MAX {
        capabilities.current_extent
    } else {
        vk::Extent2D {
            width: width.clamp(
                capabilities.min_image_extent.width,
                capabilities.max_image_extent.width,
            ),
            height: height.clamp(
                capabilities.min_image_extent.height,
                capabilities.max_image_extent.height,
            ),
        }
    }
}

fn choose_composite_alpha(supported: vk::CompositeAlphaFlagsKHR) -> vk::CompositeAlphaFlagsKHR {
    const PREFERRED: &[vk::CompositeAlphaFlagsKHR] = &[
        vk::CompositeAlphaFlagsKHR::OPAQUE,
        vk::CompositeAlphaFlagsKHR::INHERIT,
        vk::CompositeAlphaFlagsKHR::PRE_MULTIPLIED,
        vk::CompositeAlphaFlagsKHR::POST_MULTIPLIED,
    ];

    PREFERRED
        .iter()
        .copied()
        .find(|flag| supported.contains(*flag))
        .unwrap_or(vk::CompositeAlphaFlagsKHR::OPAQUE)
}
