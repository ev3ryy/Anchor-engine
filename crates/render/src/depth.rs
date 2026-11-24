use ash::{Device, Instance, vk};

use crate::Error;

pub struct DepthBuffer {
    device: Device,
    memory: vk::DeviceMemory,
    pub image: vk::Image,
    pub view: vk::ImageView,
}

impl Drop for DepthBuffer {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

pub fn choose_format(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
) -> Result<vk::Format, Error> {
    const CANDIDATES: [vk::Format; 2] = [vk::Format::D32_SFLOAT, vk::Format::D16_UNORM];

    for format in CANDIDATES {
        let props =
            unsafe { instance.get_physical_device_format_properties(physical_device, format) };
        if props
            .optimal_tiling_features
            .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT)
        {
            return Ok(format);
        }
    }

    Err(Error::NoDepthFormat)
}

pub fn create(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    device: &Device,
    extent: vk::Extent2D,
    format: vk::Format,
) -> Result<DepthBuffer, Error> {
    let image_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width: extent.width,
            height: extent.height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { device.create_image(&image_info, None)? };

    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let type_index = memory_type_index(
        instance,
        physical_device,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    let alloc_info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    let memory = match unsafe { device.allocate_memory(&alloc_info, None) } {
        Ok(memory) => memory,
        Err(err) => {
            unsafe { device.destroy_image(image, None) };
            return Err(err.into());
        }
    };

    if let Err(err) = unsafe { device.bind_image_memory(image, memory, 0) } {
        unsafe {
            device.free_memory(memory, None);
            device.destroy_image(image, None);
        }
        return Err(err.into());
    }

    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(depth_range());
    let view = match unsafe { device.create_image_view(&view_info, None) } {
        Ok(view) => view,
        Err(err) => {
            unsafe {
                device.free_memory(memory, None);
                device.destroy_image(image, None);
            }
            return Err(err.into());
        }
    };

    Ok(DepthBuffer {
        device: device.clone(),
        memory,
        image,
        view,
    })
}

fn memory_type_index(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> Result<u32, Error> {
    let memory = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    let index = (0..memory.memory_type_count).find(|&index| {
        let bit = 1 << index;
        type_bits & bit != 0
            && memory.memory_types[index as usize]
                .property_flags
                .contains(properties)
    });
    index.ok_or(Error::NoMemoryType)
}

fn depth_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::DEPTH,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}
