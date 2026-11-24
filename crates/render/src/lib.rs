mod commands;
mod device;
mod instance;
mod queue;
mod surface;
mod swapchain;

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
    NoSurfaceFormat,
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
            Self::NoSurfaceFormat => write!(f, "no surface format found"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loading(err) => Some(err),
            Self::Vulkan(err) => Some(err),
            Self::WindowHandle(err) => Some(err),
            Self::UnsupportedApiVersion(_)
            | Self::NoPhysicalDevice
            | Self::NoGraphicsQueue
            | Self::NoPresentQueue
            | Self::NoSurfaceFormat => None,
        }
    }
}

pub(crate) fn supports_vulkan_1_3(version: u32) -> bool {
    let major = vk::api_version_major(version);
    let minor = vk::api_version_minor(version);
    major > 1 || (major == 1 && minor >= 3)
}

pub struct Context {
    // Drop order: commands, swapchain, device, surface, instance. The window must outlive this context.
    commands: commands::CommandBundle,
    swapchain: swapchain::SwapchainBundle,
    device: device::DeviceBundle,
    surface: surface::SurfaceBundle,
    instance: instance::InstanceBundle,
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
        Ok(Self {
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
        let _old = std::mem::replace(&mut self.swapchain, replacement);
        Ok(())
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
