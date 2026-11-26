mod bindless;
mod camera;
mod commands;
mod depth;
mod device;
mod gpu;
mod instance;
mod instances;
mod material;
mod pipeline;
pub mod primitive;
mod queue;
mod shader;
mod surface;
mod swapchain;
mod sync;
mod texture;
mod viewport;

pub use camera::Camera;
pub use material::{MaterialDesc, MaterialId};
pub use texture::{SamplerId, TextureId};

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
    OutOfInstances,
    StaleTexture,
    StaleMaterial,
    Shader(std::io::Error),
    ShaderCompile { file: String, message: String },
    ShaderInterface(String),
    UnknownShader(String),
    Overlay(String),
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
            Self::OutOfInstances => write!(f, "instance buffer is full"),
            Self::StaleTexture => write!(f, "texture handle is stale"),
            Self::StaleMaterial => write!(f, "material handle is stale"),
            Self::Shader(err) => write!(f, "failed to read SPIR-V: {err}"),
            Self::ShaderCompile { file, message } => {
                write!(f, "failed to compile {file}: {message}")
            }
            Self::ShaderInterface(message) => write!(f, "{message}"),
            Self::UnknownShader(name) => write!(f, "unknown shader {name}"),
            Self::Overlay(message) => write!(f, "{message}"),
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
            | Self::OutOfInstances
            | Self::StaleTexture
            | Self::StaleMaterial
            | Self::ShaderCompile { .. }
            | Self::ShaderInterface(_)
            | Self::UnknownShader(_)
            | Self::Overlay(_) => None,
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

/// Where the scene is drawn. The overlay always targets the swapchain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderTarget {
    /// Scene and overlay share the window image.
    Swapchain,
    /// Scene goes to an offscreen color image of this pixel size.
    /// The overlay samples that image while drawing the window.
    Viewport([u32; 2]),
}

pub struct Overlay<'a> {
    pub device: &'a ash::Device,
    pub command_buffer: vk::CommandBuffer,
    pub color_view: vk::ImageView,
    pub extent: vk::Extent2D,
    pub load_op: vk::AttachmentLoadOp,
    /// Scene color view when the scene was drawn into a texture.
    pub scene_view: Option<vk::ImageView>,
}

pub struct Context {
    // Drop order: shaders, bindless, sync, commands, swapchain, viewport, retired viewports, depth, primitive cache, device, surface, instance.
    // The window must outlive this context. ShaderSet drops while the device is still alive,
    // and before the shared bindless pipeline layout.
    shaders: shader::ShaderSet,
    bindless: bindless::Bindless,
    sync: sync::FrameSync,
    commands: commands::CommandBundle,
    swapchain: swapchain::SwapchainBundle,
    viewport: Option<viewport::ViewportTarget>,
    retired_viewports: Vec<viewport::ViewportTarget>,
    depth: Option<depth::DepthBuffer>,
    primitives: primitive::Cache,
    draw_order: Vec<usize>,
    draw_batches: Vec<instances::Batch>,
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
        let primitives =
            primitive::Cache::create(&instance.instance, device.physical_device, &device.device)?;
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
            viewport: None,
            retired_viewports: Vec::new(),
            depth: None,
            primitives,
            draw_order: Vec::new(),
            draw_batches: Vec::new(),
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
        if self.shaders.color_format() != replacement.format {
            let depth_format = self.shaders.depth_format();
            self.shaders.recreate(replacement.format, depth_format)?;
            self.shaders.flush_retired();
        }
        let _old = std::mem::replace(&mut self.swapchain, replacement);
        Ok(())
    }

    /// Offscreen scene image sampled by the editor viewport.
    /// Recreated when `width`, `height`, or the swapchain format changes.
    /// Waits for the previous frame before replacing the image.
    pub fn scene_target(&mut self, width: u32, height: u32) -> Result<vk::ImageView, Error> {
        if width == 0 || height == 0 {
            return Err(Error::InvalidImage);
        }
        self.wait_frame()?;
        self.ensure_viewport(width, height)?;
        Ok(self.viewport.as_ref().expect("scene target").view())
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

    pub fn draw(
        &mut self,
        camera: &Camera,
        draws: &[scene::Draw],
        target: RenderTarget,
        prepare: impl FnOnce(vk::Queue) -> Result<(), Error>,
        overlay: impl FnOnce(Overlay<'_>) -> Result<(), Error>,
    ) -> Result<DrawStatus, Error> {
        self.wait_frame()?;
        prepare(self.device.graphics.handle)?;

        if self.swapchain.extent.width == 0 || self.swapchain.extent.height == 0 {
            return Ok(DrawStatus::Ok);
        }

        let kind = match target {
            RenderTarget::Swapchain => SceneKind::Swapchain,
            RenderTarget::Viewport([width, height]) if width > 0 && height > 0 => {
                self.ensure_viewport(width, height)?;
                let viewport = self.viewport.as_ref().expect("scene target");
                SceneKind::Viewport(ScenePass {
                    image: viewport.image(),
                    view: viewport.view(),
                    layout: viewport.layout,
                    extent: viewport.extent,
                    offscreen: true,
                })
            }
            RenderTarget::Viewport(_) => SceneKind::None,
        };
        let draws_scene = !matches!(kind, SceneKind::None);
        let scene_extent = match &kind {
            SceneKind::Viewport(scene) => scene.extent,
            SceneKind::Swapchain | SceneKind::None => self.swapchain.extent,
        };
        if draws_scene {
            self.ensure_depth(scene_extent)?;
        }
        let depth = self.depth.as_ref().map(|depth| DepthPass {
            image: depth.image,
            view: depth.view,
            layout: depth.layout,
        });

        self.shaders.flush_retired();
        self.bindless.flush();
        self.shaders.poll();
        if draws_scene {
            let mut order = std::mem::take(&mut self.draw_order);
            let mut batches = std::mem::take(&mut self.draw_batches);
            let index_counts = self.primitives.index_counts();
            let planned = instances::plan_draws(
                draws,
                |material| {
                    self.bindless
                        .material_index(MaterialId {
                            index: material.index,
                            generation: material.generation,
                        })
                        .ok()
                },
                |model| index_counts[primitive::slot(model) as usize],
                &mut order,
                &mut batches,
            );
            self.draw_order = order;
            self.draw_batches = batches;
            planned?;
            self.bindless.write_worlds(draws, &self.draw_order);
        } else {
            self.draw_order.clear();
            self.draw_batches.clear();
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
        let swapchain_image = self.swapchain.images[index];
        let swapchain_view = self.swapchain.views[index];
        let signal = self.swapchain.render_finished[index];
        let swapchain_extent = self.swapchain.extent;
        let swapchain_layout = self.swapchain.layouts[index];
        let scene = match kind {
            SceneKind::Swapchain => Some(ScenePass {
                image: swapchain_image,
                view: swapchain_view,
                layout: swapchain_layout,
                extent: swapchain_extent,
                offscreen: false,
            }),
            SceneKind::Viewport(scene) => Some(scene),
            SceneKind::None => None,
        };
        let drawn = if scene.is_some() {
            Some(self.shaders.pipeline("triangle", &[])?)
        } else {
            None
        };
        let frame_extent = scene
            .as_ref()
            .map(|scene| scene.extent)
            .unwrap_or(swapchain_extent);
        let mut frame = material::FramePush {
            view_proj: camera.view_proj(frame_extent).to_cols_array(),
            material_index: 0,
            instance_base: 0,
            pad1: 0,
            pad2: 0,
        };
        let batches = std::mem::take(&mut self.draw_batches);
        let meshes = self.primitives.draw_meshes();
        let descriptor_set = self.bindless.descriptor_set();
        let material_buffers = self.bindless.material_buffers();
        let image_available = self.sync.image_available;
        let in_flight = self.sync.in_flight;
        let queue = self.device.graphics.handle;
        let offscreen = scene.as_ref().is_some_and(|scene| scene.offscreen);

        let recorded = self.commands.record(|device, command_buffer| {
            record_material_visibility(device, command_buffer, material_buffers);
            record_frame(
                device,
                command_buffer,
                swapchain_image,
                swapchain_view,
                swapchain_extent,
                swapchain_layout,
                scene,
                depth,
                drawn,
                descriptor_set,
                &batches,
                &meshes,
                &mut frame,
                overlay,
            )
        });
        self.draw_batches = batches;
        recorded?;
        // The overlay rebinds the scene view before sampling it. The previous image can go now.
        self.retired_viewports.clear();

        unsafe { self.device.device.reset_fences(&[in_flight])? };
        self.commands
            .submit_frame(queue, image_available, signal, in_flight)?;
        self.swapchain.layouts[index] = vk::ImageLayout::PRESENT_SRC_KHR;
        if offscreen && let Some(viewport) = self.viewport.as_mut() {
            viewport.layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        }
        if draws_scene && let Some(depth) = self.depth.as_mut() {
            depth.layout = vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL;
        }

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

    pub fn device(&self) -> &ash::Device {
        &self.device.device
    }

    pub fn instance(&self) -> &ash::Instance {
        &self.instance.instance
    }

    pub fn color_format(&self) -> vk::Format {
        self.swapchain.format
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

    fn ensure_viewport(&mut self, width: u32, height: u32) -> Result<(), Error> {
        let format = self.swapchain.format;
        let extent = vk::Extent2D { width, height };
        if self
            .viewport
            .as_ref()
            .is_some_and(|target| target.extent == extent && target.format == format)
        {
            return Ok(());
        }
        let next = viewport::ViewportTarget::create(
            &self.instance.instance,
            self.device.physical_device,
            &self.device.device,
            extent,
            format,
        )?;
        if let Some(previous) = self.viewport.replace(next) {
            self.retired_viewports.push(previous);
        }
        Ok(())
    }

    fn ensure_depth(&mut self, extent: vk::Extent2D) -> Result<(), Error> {
        if self
            .depth
            .as_ref()
            .is_some_and(|depth| depth.extent == extent)
        {
            return Ok(());
        }
        let format = self.shaders.depth_format();
        self.depth = create_depth(
            &self.instance.instance,
            self.device.physical_device,
            &self.device.device,
            extent,
            format,
        )?;
        Ok(())
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

#[derive(Clone, Copy)]
enum SceneKind {
    Swapchain,
    Viewport(ScenePass),
    None,
}

#[derive(Clone, Copy)]
struct ScenePass {
    image: vk::Image,
    view: vk::ImageView,
    layout: vk::ImageLayout,
    extent: vk::Extent2D,
    offscreen: bool,
}

#[derive(Clone, Copy)]
struct DepthPass {
    image: vk::Image,
    view: vk::ImageView,
    layout: vk::ImageLayout,
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

fn record_geometry_visibility(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    meshes: &[primitive::DrawMesh],
) {
    if meshes.is_empty() {
        return;
    }
    let mut buffers = Vec::with_capacity(meshes.len() * 2);
    for mesh in meshes {
        buffers.push(mesh.vertex);
        buffers.push(mesh.index);
    }
    let barriers: Vec<_> = buffers
        .iter()
        .map(|buffer| {
            vk::BufferMemoryBarrier2::default()
                .src_stage_mask(vk::PipelineStageFlags2::HOST)
                .src_access_mask(vk::AccessFlags2::HOST_WRITE)
                .dst_stage_mask(vk::PipelineStageFlags2::VERTEX_ATTRIBUTE_INPUT)
                .dst_access_mask(
                    vk::AccessFlags2::VERTEX_ATTRIBUTE_READ | vk::AccessFlags2::INDEX_READ,
                )
                .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
                .buffer(*buffer)
                .offset(0)
                .size(vk::WHOLE_SIZE)
        })
        .collect();
    let dependency = vk::DependencyInfo::default().buffer_memory_barriers(&barriers);
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dependency) };
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
    swapchain_image: vk::Image,
    swapchain_view: vk::ImageView,
    swapchain_extent: vk::Extent2D,
    swapchain_layout: vk::ImageLayout,
    scene: Option<ScenePass>,
    depth: Option<DepthPass>,
    drawn: Option<shader::DrawBinding>,
    descriptor_set: vk::DescriptorSet,
    batches: &[instances::Batch],
    meshes: &[primitive::DrawMesh],
    frame: &mut material::FramePush,
    overlay: impl FnOnce(Overlay<'_>) -> Result<(), Error>,
) -> Result<(), Error> {
    let color_range = color_range();
    if let Some(scene) = scene {
        let Some(depth) = depth else {
            return Err(Error::NoDepthFormat);
        };
        let Some(drawn) = drawn else {
            return Err(Error::UnknownShader("triangle".to_string()));
        };
        let (src_stage, src_access) = color_source(scene.layout);
        let to_color = image_barrier(
            scene.image,
            color_range,
            src_stage,
            src_access,
            vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
            scene.layout,
            vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        );
        let (depth_stage, depth_access) = depth_source(depth.layout);
        let to_depth = image_barrier(
            depth.image,
            depth_range(),
            depth_stage,
            depth_access,
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
            depth.layout,
            vk::ImageLayout::DEPTH_ATTACHMENT_OPTIMAL,
        );
        cmd_barriers(device, command_buffer, &[to_color, to_depth]);
        record_scene(
            device,
            command_buffer,
            scene,
            depth.view,
            drawn,
            descriptor_set,
            batches,
            meshes,
            frame,
        )?;
        if scene.offscreen {
            let to_sample = image_barrier(
                scene.image,
                color_range,
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                vk::PipelineStageFlags2::FRAGMENT_SHADER,
                vk::AccessFlags2::SHADER_SAMPLED_READ,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
            let to_overlay =
                swapchain_color_barrier(swapchain_image, color_range, swapchain_layout);
            cmd_barriers(device, command_buffer, &[to_sample, to_overlay]);
        } else {
            let between = image_barrier(
                swapchain_image,
                color_range,
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags2::COLOR_ATTACHMENT_READ | vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
            );
            cmd_barriers(device, command_buffer, &[between]);
        }
    } else {
        cmd_barriers(
            device,
            command_buffer,
            &[swapchain_color_barrier(
                swapchain_image,
                color_range,
                swapchain_layout,
            )],
        );
    }

    let offscreen = scene.is_some_and(|scene| scene.offscreen);
    overlay(Overlay {
        device,
        command_buffer,
        color_view: swapchain_view,
        extent: swapchain_extent,
        load_op: if offscreen || scene.is_none() {
            vk::AttachmentLoadOp::CLEAR
        } else {
            vk::AttachmentLoadOp::LOAD
        },
        scene_view: scene
            .filter(|scene| scene.offscreen)
            .map(|scene| scene.view),
    })?;

    let to_present = image_barrier(
        swapchain_image,
        color_range,
        vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        vk::PipelineStageFlags2::NONE,
        vk::AccessFlags2::NONE,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        vk::ImageLayout::PRESENT_SRC_KHR,
    );
    cmd_barriers(device, command_buffer, &[to_present]);
    Ok(())
}

fn record_scene(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    scene: ScenePass,
    depth_view: vk::ImageView,
    drawn: shader::DrawBinding,
    descriptor_set: vk::DescriptorSet,
    batches: &[instances::Batch],
    meshes: &[primitive::DrawMesh],
    frame: &mut material::FramePush,
) -> Result<(), Error> {
    if drawn.push_size as usize != std::mem::size_of::<material::FramePush>() {
        return Err(Error::ShaderInterface(format!(
            "frame push is {} bytes, shader contract is {}",
            std::mem::size_of::<material::FramePush>(),
            drawn.push_size
        )));
    }
    let frame_bytes = unsafe {
        std::slice::from_raw_parts(
            (frame as *const material::FramePush).cast::<u8>(),
            std::mem::size_of::<material::FramePush>(),
        )
    };
    let attachment = vk::RenderingAttachmentInfo::default()
        .image_view(scene.view)
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
            extent: scene.extent,
        })
        .layer_count(1)
        .color_attachments(std::slice::from_ref(&attachment))
        .depth_attachment(&depth_attachment);
    record_geometry_visibility(device, command_buffer, meshes);
    unsafe {
        device.cmd_begin_rendering(command_buffer, &rendering);
        device.cmd_bind_pipeline(
            command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            drawn.pipeline,
        );
        device.cmd_bind_descriptor_sets(
            command_buffer,
            vk::PipelineBindPoint::GRAPHICS,
            drawn.layout,
            0,
            &[descriptor_set],
            &[],
        );
        device.cmd_push_constants(
            command_buffer,
            drawn.layout,
            drawn.push_stages,
            drawn.push_offset,
            frame_bytes,
        );
        device.cmd_set_viewport(
            command_buffer,
            0,
            &[vk::Viewport {
                x: 0.0,
                y: 0.0,
                width: scene.extent.width as f32,
                height: scene.extent.height as f32,
                min_depth: 0.0,
                max_depth: 1.0,
            }],
        );
        device.cmd_set_scissor(
            command_buffer,
            0,
            &[vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: scene.extent,
            }],
        );
        for batch in batches {
            let mesh = meshes[batch.slot as usize];
            frame.material_index = batch.material_index;
            frame.instance_base = batch.begin;
            let tail =
                std::slice::from_raw_parts((&frame.material_index as *const u32).cast::<u8>(), 8);
            device.cmd_bind_vertex_buffers(command_buffer, 0, &[mesh.vertex], &[0]);
            device.cmd_bind_index_buffer(command_buffer, mesh.index, 0, vk::IndexType::UINT32);
            device.cmd_push_constants(
                command_buffer,
                drawn.layout,
                drawn.push_stages,
                drawn.push_offset + 64,
                tail,
            );
            device.cmd_draw_indexed(command_buffer, batch.index_count, batch.count, 0, 0, 0);
        }
        device.cmd_end_rendering(command_buffer);
    }
    Ok(())
}

fn swapchain_color_barrier(
    image: vk::Image,
    range: vk::ImageSubresourceRange,
    layout: vk::ImageLayout,
) -> vk::ImageMemoryBarrier2<'static> {
    let (src_stage, src_access) = color_source(layout);
    image_barrier(
        image,
        range,
        src_stage,
        src_access,
        vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
        vk::AccessFlags2::COLOR_ATTACHMENT_WRITE,
        layout,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    )
}

fn color_source(layout: vk::ImageLayout) -> (vk::PipelineStageFlags2, vk::AccessFlags2) {
    if layout == vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL {
        (
            vk::PipelineStageFlags2::FRAGMENT_SHADER,
            vk::AccessFlags2::SHADER_SAMPLED_READ,
        )
    } else {
        (
            vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT,
            vk::AccessFlags2::NONE,
        )
    }
}

fn depth_source(layout: vk::ImageLayout) -> (vk::PipelineStageFlags2, vk::AccessFlags2) {
    if layout == vk::ImageLayout::UNDEFINED {
        (vk::PipelineStageFlags2::NONE, vk::AccessFlags2::NONE)
    } else {
        (
            vk::PipelineStageFlags2::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags2::LATE_FRAGMENT_TESTS,
            vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_READ
                | vk::AccessFlags2::DEPTH_STENCIL_ATTACHMENT_WRITE,
        )
    }
}

fn image_barrier(
    image: vk::Image,
    range: vk::ImageSubresourceRange,
    src_stage: vk::PipelineStageFlags2,
    src_access: vk::AccessFlags2,
    dst_stage: vk::PipelineStageFlags2,
    dst_access: vk::AccessFlags2,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) -> vk::ImageMemoryBarrier2<'static> {
    vk::ImageMemoryBarrier2::default()
        .src_stage_mask(src_stage)
        .src_access_mask(src_access)
        .dst_stage_mask(dst_stage)
        .dst_access_mask(dst_access)
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(range)
}

fn cmd_barriers(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    barriers: &[vk::ImageMemoryBarrier2],
) {
    let dependency = vk::DependencyInfo::default().image_memory_barriers(barriers);
    unsafe { device.cmd_pipeline_barrier2(command_buffer, &dependency) };
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

fn depth_range() -> vk::ImageSubresourceRange {
    vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::DEPTH,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    }
}
