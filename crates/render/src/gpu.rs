use ash::{Device, Instance, vk};

use crate::Error;

pub struct GpuBuffer {
    device: Device,
    memory: vk::DeviceMemory,
    pub buffer: vk::Buffer,
    size: vk::DeviceSize,
    mapped: *mut u8,
}

impl Drop for GpuBuffer {
    fn drop(&mut self) {
        unsafe {
            if !self.mapped.is_null() {
                self.device.unmap_memory(self.memory);
            }
            self.device.destroy_buffer(self.buffer, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

impl GpuBuffer {
    pub fn mapped(&self) -> *mut u8 {
        self.mapped
    }

    pub fn size(&self) -> vk::DeviceSize {
        self.size
    }

    pub fn host_visible(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
    ) -> Result<Self, Error> {
        Self::create(
            instance,
            physical_device,
            device,
            size,
            usage,
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
            true,
        )
    }
}

pub struct GpuImage {
    device: Device,
    memory: vk::DeviceMemory,
    image: vk::Image,
    pub view: vk::ImageView,
}

impl Drop for GpuImage {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_image_view(self.view, None);
            self.device.destroy_image(self.image, None);
            self.device.free_memory(self.memory, None);
        }
    }
}

impl GpuImage {
    pub fn sampled(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        extent: vk::Extent2D,
        format: vk::Format,
    ) -> Result<Self, Error> {
        let props =
            unsafe { instance.get_physical_device_format_properties(physical_device, format) };
        let needed = vk::FormatFeatureFlags::SAMPLED_IMAGE | vk::FormatFeatureFlags::TRANSFER_DST;
        if !props.optimal_tiling_features.contains(needed) {
            return Err(Error::UnsupportedFormat);
        }

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
            .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        let image = unsafe { device.create_image(&image_info, None)? };
        let requirements = unsafe { device.get_image_memory_requirements(image) };
        let memory = match allocate(
            instance,
            physical_device,
            device,
            requirements,
            vk::MemoryPropertyFlags::DEVICE_LOCAL,
        ) {
            Ok(memory) => memory,
            Err(err) => {
                unsafe { device.destroy_image(image, None) };
                return Err(err);
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
            .subresource_range(color_range());
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
        Ok(Self {
            device: device.clone(),
            memory,
            image,
            view,
        })
    }

    pub fn image(&self) -> vk::Image {
        self.image
    }
}

pub fn record_image_upload(
    device: &Device,
    command_buffer: vk::CommandBuffer,
    staging: vk::Buffer,
    image: vk::Image,
    extent: vk::Extent2D,
) {
    let range = color_range();
    let to_copy = vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::NONE)
        .src_access_mask(vk::AccessFlags2::NONE)
        .dst_stage_mask(vk::PipelineStageFlags2::COPY)
        .dst_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    let prepare =
        vk::DependencyInfo::default().image_memory_barriers(std::slice::from_ref(&to_copy));
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &prepare) };

    let region = vk::BufferImageCopy::default()
        .image_subresource(vk::ImageSubresourceLayers {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            mip_level: 0,
            base_array_layer: 0,
            layer_count: 1,
        })
        .image_extent(vk::Extent3D {
            width: extent.width,
            height: extent.height,
            depth: 1,
        });
    unsafe {
        device.cmd_copy_buffer_to_image(
            command_buffer,
            staging,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &[region],
        );
    }

    let to_sample = vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::COPY)
        .src_access_mask(vk::AccessFlags2::TRANSFER_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags2::FRAGMENT_SHADER)
        .dst_access_mask(vk::AccessFlags2::SHADER_SAMPLED_READ)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    let publish =
        vk::DependencyInfo::default().image_memory_barriers(std::slice::from_ref(&to_sample));
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &publish) };
}

impl GpuBuffer {
    fn create(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        size: vk::DeviceSize,
        usage: vk::BufferUsageFlags,
        properties: vk::MemoryPropertyFlags,
        mapped: bool,
    ) -> Result<Self, Error> {
        let info = vk::BufferCreateInfo::default()
            .size(size)
            .usage(usage)
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        let buffer = unsafe { device.create_buffer(&info, None)? };
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory = match allocate(instance, physical_device, device, requirements, properties) {
            Ok(memory) => memory,
            Err(err) => {
                unsafe { device.destroy_buffer(buffer, None) };
                return Err(err);
            }
        };
        if let Err(err) = unsafe { device.bind_buffer_memory(buffer, memory, 0) } {
            unsafe {
                device.free_memory(memory, None);
                device.destroy_buffer(buffer, None);
            }
            return Err(err.into());
        }
        let mapped = if mapped {
            match unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) } {
                Ok(pointer) => {
                    let pointer = pointer.cast::<u8>();
                    unsafe { pointer.write_bytes(0, size as usize) };
                    pointer
                }
                Err(err) => {
                    unsafe {
                        device.free_memory(memory, None);
                        device.destroy_buffer(buffer, None);
                    }
                    return Err(err.into());
                }
            }
        } else {
            std::ptr::null_mut()
        };
        Ok(Self {
            device: device.clone(),
            memory,
            buffer,
            size,
            mapped,
        })
    }
}

fn allocate(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    device: &Device,
    requirements: vk::MemoryRequirements,
    properties: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory, Error> {
    let type_index = memory_type_index(
        instance,
        physical_device,
        requirements.memory_type_bits,
        properties,
    )?;
    let info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(type_index);
    unsafe { device.allocate_memory(&info, None).map_err(Error::from) }
}

fn memory_type_index(
    instance: &Instance,
    physical_device: vk::PhysicalDevice,
    type_bits: u32,
    properties: vk::MemoryPropertyFlags,
) -> Result<u32, Error> {
    let memory = unsafe { instance.get_physical_device_memory_properties(physical_device) };
    (0..memory.memory_type_count)
        .find(|&index| {
            type_bits & (1 << index) != 0
                && memory.memory_types[index as usize]
                    .property_flags
                    .contains(properties)
        })
        .ok_or(Error::NoMemoryType)
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
