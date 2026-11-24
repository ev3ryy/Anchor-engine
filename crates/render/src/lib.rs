mod commands;
mod device;
mod instance;
mod pipeline;
mod queue;
mod surface;
mod swapchain;
mod sync;

pub use ash;

use ash::vk;
use raw_window_handle::{HandleError, HasDisplayHandle, HasWindowHandle};

#[derive(Debug)]
pub enum Error {
    Loading(ash::LoadingError),
    Vulkan(vk::Result),
    WindowHandle(HandleError),
    UnsupportedApiVersion(u32),
    NoPhysicalDevice,
    NoGraphicsQueue,
    NoPresentQueue,
    NoMaintenance5,
    NoSurfaceFormat,
    Shader(std::io::Error),
}

impl From<vk::Result> for Error {
    fn from(result: vk::Result) -> Self {
        Self::Vulkan(result)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Loading(err) => write!(f, "failed to load Vulkan: {err}"),
            Self::Vulkan(err) => write!(f, "Vulkan error: {err}"),
            Self::WindowHandle(err) => write!(f, "invalid window handle: {err}"),
            Self::UnsupportedApiVersion(version) => {
                let major = vk::api_version_major(*version);
                let minor = vk::api_version_minor(*version);
                write!(f, "Vulkan 1.3 required, found {major}.{minor}")
            }
            Self::NoPhysicalDevice => write!(f, "no suitable physical device found"),
            Self::NoGraphicsQueue => write!(f, "no graphics queue family found"),
            Self::NoPresentQueue => write!(f, "no graphics queue supports presentation"),
            Self::NoMaintenance5 => write!(f, "no device supports VK_KHR_maintenance5"),
            Self::NoSurfaceFormat => write!(f, "no surface format found"),
            Self::Shader(err) => write!(f, "failed to read SPIR-V: {err}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loading(err) => Some(err),
            Self::Vulkan(err) => Some(err),
            Self::WindowHandle(err) => Some(err),
            Self::Shader(err) => Some(err),
            Self::UnsupportedApiVersion(_)
            | Self::NoPhysicalDevice
            | Self::NoGraphicsQueue
            | Self::NoPresentQueue
            | Self::NoMaintenance5
            | Self::NoSurfaceFormat => None,
        }
    }
}

pub(crate) fn supports_vulkan_1_3(version: u32) -> bool {
    let major = vk::api_version_major(version);
    let minor = vk::api_version_minor(version);
    major > 1 || (major == 1 && minor >= 3)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DrawStatus {
    Ok,
    OutOfDate,
}

pub struct Context {
    // Drop order: pipeline, sync, commands, swapchain, device, surface, instance.
    // The window must outlive this context.
    pipeline: pipeline::PipelineBundle,
    sync: sync::FrameSync,
    commands: commands::CommandBundle,
    swapchain: swapchain::SwapchainBundle,
    device: device::DeviceBundle,
    surface: surface::SurfaceBundle,
    instance: instance::InstanceBundle,
}

impl Drop for Context {
    fn drop(&mut self) {
        unsafe {
            let _ = self.device.device.device_wait_idle();
        }
    }
}

impl Context {
    pub fn new(
        window: &(impl HasDisplayHandle + HasWindowHandle),
        width: u32,
        height: u32,
    ) -> Result<Self, Error> {
        let instance = instance::create(window)?;
        let surface = surface::create(&instance.entry, &instance.instance, window)?;
        let device = device::create(&instance.instance, &surface.loader, surface.surface)?;
        let commands = commands::create(&device.device, device.graphics.family)?;
        let swapchain = swapchain::create(
            &instance.instance,
            &device.device,
            device.physical_device,
            &surface.loader,
            surface.surface,
            width,
            height,
            vk::SwapchainKHR::null(),
        )?;
        let sync = sync::create(&device.device)?;
        let pipeline = pipeline::create(&device.device, swapchain.format)?;
        Ok(Self {
            pipeline,
            sync,
            commands,
            swapchain,
            device,
            surface,
            instance,
        })
    }

    pub fn resize(&mut self, width: u32, height: u32) -> Result<(), Error> {
        if width == 0 || height == 0 {
            return Ok(());
        }

        unsafe { self.device.device.device_wait_idle() }?;
        let replacement = swapchain::create(
            &self.instance.instance,
            &self.device.device,
            self.device.physical_device,
            &self.surface.loader,
            self.surface.surface,
            width,
            height,
            self.swapchain.swapchain,
        )?;
        if self.pipeline.format() != replacement.format {
            self.pipeline = pipeline::create(&self.device.device, replacement.format)?;
        }
        let _old = std::mem::replace(&mut self.swapchain, replacement);
        Ok(())
    }

    pub fn draw(&mut self) -> Result<DrawStatus, Error> {
        if self.swapchain.extent.width == 0 || self.swapchain.extent.height == 0 {
            return Ok(DrawStatus::Ok);
        }

        unsafe {
            self.device
                .device
                .wait_for_fences(&[self.sync.in_flight], true, u64::MAX)?;
        }

        let acquire = unsafe {
            self.swapchain.loader.acquire_next_image(
                self.swapchain.swapchain,
                u64::MAX,
                self.sync.image_available,
                vk::Fence::null(),
            )
        };
        let (image_index, suboptimal) = match acquire {
            Ok((index, suboptimal)) => (index, suboptimal),
            Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(DrawStatus::OutOfDate),
            Err(err) => return Err(err.into()),
        };

        let index = image_index as usize;
        let image = self.swapchain.images[index];
        let view = self.swapchain.views[index];
        let signal = self.swapchain.render_finished[index];
        let extent = self.swapchain.extent;
        let pipeline = self.pipeline.pipeline;
        let image_available = self.sync.image_available;
        let in_flight = self.sync.in_flight;
        let queue = self.device.graphics.handle;

        self.commands.record(|device, command_buffer| {
            record_triangle(device, command_buffer, image, view, extent, pipeline)
        })?;

        unsafe { self.device.device.reset_fences(&[in_flight])? };
        self.commands
            .submit_frame(queue, image_available, signal, in_flight)?;

        let wait_semaphores = [signal];
        let swapchains = [self.swapchain.swapchain];
        let image_indices = [image_index];
        let present_info = vk::PresentInfoKHR::default()
            .wait_semaphores(&wait_semaphores)
            .swapchains(&swapchains)
            .image_indices(&image_indices);
        let present_suboptimal =
            match unsafe { self.swapchain.loader.queue_present(queue, &present_info) } {
                Ok(suboptimal) => suboptimal,
                Err(vk::Result::ERROR_OUT_OF_DATE_KHR) => return Ok(DrawStatus::OutOfDate),
                Err(err) => return Err(err.into()),
            };

        if suboptimal || present_suboptimal {
            Ok(DrawStatus::OutOfDate)
        } else {
            Ok(DrawStatus::Ok)
        }
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.device.physical_device
    }

    pub fn graphics_queue_family(&self) -> u32 {
        self.device.graphics.family
    }

    pub fn graphics_queue(&self) -> vk::Queue {
        self.device.graphics.handle
    }

    pub fn submit(
        &mut self,
        record: impl FnOnce(&ash::Device, vk::CommandBuffer) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.commands.submit(self.device.graphics.handle, record)
    }
}

fn record_triangle(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    view: vk::ImageView,
    extent: vk::Extent2D,
    pipeline: vk::Pipeline,
) -> Result<(), Error> {
    let range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let to_color = vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags2::NONE)
        .dst_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    let to_color =
        vk::DependencyInfo::default().image_memory_barriers(std::slice::from_ref(&to_color));
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &to_color) };

    let attachment = vk::RenderingAttachmentInfo::default()
        .image_view(view)
        .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .clear_value(vk::ClearValue {
            color: vk::ClearColorValue {
                float32: [0.02, 0.02, 0.02, 1.0],
            },
        });
    let rendering = vk::RenderingInfo::default()
        .render_area(vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        })
        .layer_count(1)
        .color_attachments(std::slice::from_ref(&attachment));
    unsafe {
        device.cmd_begin_rendering(command_buffer, &rendering);
        device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline);
        device.cmd_set_viewport(
            command_buffer,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: extent.width as f32,
                height: extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        device.cmd_set_scissor(
            command_buffer,
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent,
            }],
        );
        device.cmd_draw(command_buffer, 3, 1, 0, 0);
        device.cmd_end_rendering(command_buffer);
    }

    let to_present = vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags2::COLOR_ATTACHMENT_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags2::NONE)
        .dst_access_mask(vk::AccessFlags2::NONE)
        .old_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .new_layout(vk::ImageLayout::PRESENT_SRC_KHR)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range);
    let to_present =
        vk::DependencyInfo::default().image_memory_barriers(std::slice::from_ref(&to_present));
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &to_present) };
    Ok(())
}
