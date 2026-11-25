mod bindless;
mod camera;
mod commands;
mod depth;
mod device;
mod gpu;
mod instance;
mod material;
mod pipeline;
mod queue;
mod shader;
mod surface;
mod swapchain;
mod sync;

pub use camera::Camera;
pub use material::{MaterialDesc, MaterialId, SamplerId, TextureId};

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
    NoBindless,
    NoSurfaceFormat,
    NoDepthFormat,
    NoMemoryType,
    UnsupportedFormat,
    InvalidImage,
    OutOfTextures,
    OutOfMaterials,
    StaleTexture,
    StaleMaterial,
    Shader(std::io::Error),
    ShaderCompile { file: String, message: String },
    ShaderInterface(String),
    UnknownShader(String),
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
            Self::NoBindless => write!(f, "no device supports bindless materials"),
            Self::NoSurfaceFormat => write!(f, "no surface format found"),
            Self::NoDepthFormat => write!(f, "no supported depth format found"),
            Self::NoMemoryType => write!(f, "no suitable device memory type found"),
            Self::UnsupportedFormat => write!(f, "texture format is not supported"),
            Self::InvalidImage => write!(f, "texture byte length does not match its extent"),
            Self::OutOfTextures => write!(f, "bindless texture heap is full"),
            Self::OutOfMaterials => write!(f, "material table is full"),
            Self::StaleTexture => write!(f, "texture handle is stale"),
            Self::StaleMaterial => write!(f, "material handle is stale"),
            Self::Shader(err) => write!(f, "failed to read SPIR-V: {err}"),
            Self::ShaderCompile { file, message } => {
                write!(f, "failed to compile {file}: {message}")
            }
            Self::ShaderInterface(message) => write!(f, "{message}"),
            Self::UnknownShader(name) => write!(f, "unknown shader {name}"),
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
            | Self::NoBindless
            | Self::NoSurfaceFormat
            | Self::NoDepthFormat
            | Self::NoMemoryType
            | Self::UnsupportedFormat
            | Self::InvalidImage
            | Self::OutOfTextures
            | Self::OutOfMaterials
            | Self::StaleTexture
            | Self::StaleMaterial
            | Self::ShaderCompile { .. }
            | Self::ShaderInterface(_)
            | Self::UnknownShader(_) => None,
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
    // Drop order: shaders, bindless, sync, commands, swapchain, depth, device, surface, instance.
    // The window must outlive this context. ShaderSet drops while the device is still alive,
    // and before the shared bindless pipeline layout.
    shaders: shader::ShaderSet,
    bindless: bindless::Bindless,
    sync: sync::FrameSync,
    commands: commands::CommandBundle,
    swapchain: swapchain::SwapchainBundle,
    depth: Option<depth::DepthBuffer>,
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
        let depth_format = depth::choose_format(&instance.instance, device.physical_device)?;
        let depth = create_depth(
            &instance.instance,
            device.physical_device,
            &device.device,
            swapchain.extent,
            depth_format,
        )?;
        let mut bindless = bindless::Bindless::allocate(
            &instance.instance,
            device.physical_device,
            &device.device,
            device.texture_capacity,
        )?;
        bindless.prepare_white()?;
        let (staging, image, upload_extent) = bindless.upload_source();
        if let Err(err) = commands.submit(device.graphics.handle, move |device, command_buffer| {
            gpu::record_image_upload(device, command_buffer, staging, image, upload_extent);
            Ok(())
        }) {
            bindless.cancel_upload();
            return Err(err);
        }
        let _white = bindless.finish_upload()?;
        let shaders = shader::ShaderSet::new(
            &instance.instance,
            device.physical_device,
            &device.device,
            swapchain.format,
            depth_format,
            bindless.pipeline_layout(),
        )?;
        Ok(Self {
            shaders,
            bindless,
            sync,
            commands,
            swapchain,
            depth,
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
        self.shaders.flush_retired();
        self.bindless.flush();
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
        let depth_format = self.shaders.depth_format();
        let depth = create_depth(
            &self.instance.instance,
            self.device.physical_device,
            &self.device.device,
            replacement.extent,
            depth_format,
        )?;
        if self.shaders.color_format() != replacement.format {
            self.shaders.recreate(replacement.format, depth_format)?;
            self.shaders.flush_retired();
        }
        let _old = std::mem::replace(&mut self.swapchain, replacement);
        self.depth = depth;
        Ok(())
    }

    pub fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<TextureId, Error> {
        self.wait_frame()?;
        self.shaders.flush_retired();
        self.bindless.flush();
        self.bindless.prepare_texture(width, height, pixels)?;
        let (staging, image, extent) = self.bindless.upload_source();
        if let Err(err) = self.commands.submit(
            self.device.graphics.handle,
            move |device, command_buffer| {
                gpu::record_image_upload(device, command_buffer, staging, image, extent);
                Ok(())
            },
        ) {
            self.bindless.cancel_upload();
            return Err(err);
        }
        self.bindless.finish_upload()
    }

    pub fn free_texture(&mut self, id: TextureId) -> Result<(), Error> {
        self.bindless.free_texture(id)
    }

    pub fn create_material(&mut self, desc: MaterialDesc) -> Result<MaterialId, Error> {
        self.bindless.create_material(desc)
    }

    pub fn update_material(&mut self, id: MaterialId, desc: MaterialDesc) -> Result<(), Error> {
        self.bindless.update_material(id, desc)
    }

    pub fn free_material(&mut self, id: MaterialId) -> Result<(), Error> {
        self.bindless.free_material(id)
    }

    pub fn draw(&mut self, camera: &Camera, material: MaterialId) -> Result<DrawStatus, Error> {
        if self.swapchain.extent.width == 0 || self.swapchain.extent.height == 0 {
            return Ok(DrawStatus::Ok);
        }
        let Some((depth_image, depth_view)) =
            self.depth.as_ref().map(|depth| (depth.image, depth.view))
        else {
            return Ok(DrawStatus::Ok);
        };

        self.wait_frame()?;
        self.shaders.flush_retired();
        self.bindless.flush();
        self.shaders.poll();
        let material_index = self.bindless.material_index(material)?;

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
        let drawn = self.shaders.pipeline("triangle", &[])?;
        let frame = material::FramePush {
            view_proj: camera.view_proj(extent).to_cols_array(),
            material_index,
            pad0: 0,
            pad1: 0,
            pad2: 0,
        };
        let descriptor_set = self.bindless.descriptor_set();
        let material_buffers = self.bindless.material_buffers();
        let image_available = self.sync.image_available;
        let in_flight = self.sync.in_flight;
        let queue = self.device.graphics.handle;

        self.commands.record(|device, command_buffer| {
            record_material_visibility(device, command_buffer, material_buffers);
            record_frame(
                device,
                command_buffer,
                image,
                view,
                depth_image,
                depth_view,
                extent,
                drawn.pipeline,
                drawn.layout,
                drawn.push_stages,
                drawn.push_offset,
                drawn.push_size,
                descriptor_set,
                &frame,
            )
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

    fn wait_frame(&self) -> Result<(), Error> {
        unsafe {
            self.device
                .device
                .wait_for_fences(&[self.sync.in_flight], true, u64::MAX)?;
        }
        Ok(())
    }

    pub fn submit(
        &mut self,
        record: impl FnOnce(&ash::Device, vk::CommandBuffer) -> Result<(), Error>,
    ) -> Result<(), Error> {
        self.commands.submit(self.device.graphics.handle, record)
    }
}

fn create_depth(
    instance: &ash::Instance,
    physical_device: vk::PhysicalDevice,
    device: &ash::Device,
    extent: vk::Extent2D,
    format: vk::Format,
) -> Result<Option<depth::DepthBuffer>, Error> {
    if extent.width == 0 || extent.height == 0 {
        return Ok(None);
    }
    depth::create(instance, physical_device, device, extent, format).map(Some)
}

fn record_material_visibility(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    buffers: [vk::Buffer; 2],
) {
    let barriers = buffers.map(|buffer| {
        vk::BufferMemoryBarrier2::default()
            .src_stage_mask(vk::PipelineStageFlags2::HOST)
            .src_access_mask(vk::AccessFlags2::HOST_WRITE)
            .dst_stage_mask(
                vk::PipelineStageFlags2::VERTEX_SHADER | vk::PipelineStageFlags2::FRAGMENT_SHADER,
            )
            .dst_access_mask(vk::AccessFlags2::SHADER_STORAGE_READ)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .buffer(buffer)
            .offset(0)
            .size(vk::WHOLE_SIZE)
    });
    let dependency = vk::DependencyInfo::default().buffer_memory_barriers(&barriers);
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dependency) };
}

fn record_frame(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    view: vk::ImageView,
    depth_image: vk::Image,
    depth_view: vk::ImageView,
    extent: vk::Extent2D,
    pipeline: vk::Pipeline,
    layout: vk::PipelineLayout,
    push_stages: vk::ShaderStageFlags,
    push_offset: u32,
    push_size: u32,
    descriptor_set: vk::DescriptorSet,
    frame: &material::FramePush,
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
    let depth_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::DEPTH,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let to_depth = vk::ImageMemoryBarrier2::default()
        .src_stage_mask(vk::PipelineStageFlags2::NONE)
        .src_access_mask(vk::AccessFlags2::NONE)
        .dst_stage_mask(
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
        )
        .dst_access_mask(
            vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        )
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(depth_image)
        .subresource_range(depth_range);
    let barriers = [to_color, to_depth];
    let dependency = vk::DependencyInfo::default().image_memory_barriers(&barriers);
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dependency) };

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
    let depth_attachment = vk::RenderingAttachmentInfo::default()
        .image_view(depth_view)
        .image_layout(vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::DONT_CARE)
        .clear_value(vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: 1.0,
                stencil: 0,
            },
        });
    let rendering = vk::RenderingInfo::default()
        .render_area(vk::Rect2D {
            offset: vk::Offset2D::default(),
            extent,
        })
        .layer_count(1)
        .color_attachments(std::slice::from_ref(&attachment))
        .depth_attachment(&depth_attachment);
    if push_size as usize != std::mem::size_of::<material::FramePush>() {
        return Err(Error::ShaderInterface(format!(
            "frame push is {} bytes, shader contract is {push_size}",
            std::mem::size_of::<material::FramePush>()
        )));
    }
    let frame_bytes = unsafe {
        std::slice::from_raw_parts(
            (frame as *const material::FramePush).cast::<u8>(),
            push_size as usize,
        )
    };
    let mut bound_layout = vk::PipelineLayout::null();
    unsafe {
        device.cmd_begin_rendering(command_buffer, &rendering);
        device.cmd_bind_pipeline(command_buffer, vk::PipelineBindPoint::GRAPHICS, pipeline);
        if bound_layout != layout {
            device.cmd_bind_descriptor_sets(
                command_buffer,
                vk::PipelineBindPoint::GRAPHICS,
                layout,
                0,
                &[descriptor_set],
                &[],
            );
            bound_layout = layout;
        }
        debug_assert_eq!(bound_layout, layout);
        device.cmd_push_constants(
            command_buffer,
            layout,
            push_stages,
            push_offset,
            frame_bytes,
        );
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
        device.cmd_draw(command_buffer, 36, 1, 0, 0);
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
