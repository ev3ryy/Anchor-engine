use ash::{Device, Instance, khr::swapchain, vk};

use crate::Error;

pub struct SwapchainBundle {
    device: Device,
    pub loader: swapchain::Device,
    pub swapchain: vk::SwapchainKHR,
    pub images: Vec<vk::Image>,
    pub views: Vec<vk::ImageView>,
    pub render_finished: Vec<vk::Semaphore>,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
}

impl Drop for SwapchainBundle {
    fn drop(&mut self) {
        unsafe {
            for &semaphore in &self.render_finished {
                self.device.destroy_semaphore(semaphore, None);
            }
            for &view in &self.views {
                self.device.destroy_image_view(view, None);
            }
            if self.swapchain != vk::SwapchainKHR::null() {
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
    let images = match unsafe { loader.get_swapchain_images(swapchain) } {
        Ok(images) => images,
        Err(err) => {
            unsafe { loader.destroy_swapchain(swapchain, None) };
            return Err(err.into());
        }
    };

    let mut views = Vec::with_capacity(images.len());
    let mut render_finished = Vec::with_capacity(images.len());
    for &image in &images {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(color_range());
        match unsafe { device.create_image_view(&view_info, None) } {
            Ok(view) => views.push(view),
            Err(err) => {
                destroy_partial(device, &loader, swapchain, &views, &render_finished);
                return Err(err.into());
            }
        }

        match unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None) } {
            Ok(semaphore) => render_finished.push(semaphore),
            Err(err) => {
                destroy_partial(device, &loader, swapchain, &views, &render_finished);
                return Err(err.into());
            }
        }
    }

    Ok(SwapchainBundle {
        device: device.clone(),
        loader,
        swapchain,
        images,
        views,
        render_finished,
        format,
        extent,
    })
}

fn destroy_partial(
    device: &Device,
    loader: &swapchain::Device,
    swapchain: vk::SwapchainKHR,
    views: &[vk::ImageView],
    render_finished: &[vk::Semaphore],
) {
    unsafe {
        for &semaphore in render_finished {
            device.destroy_semaphore(semaphore, None);
        }
        for &view in views {
            device.destroy_image_view(view, None);
        }
        if swapchain != vk::SwapchainKHR::null() {
            loader.destroy_swapchain(swapchain, None);
        }
    }
}

fn color_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
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
