mod device;
mod instance;

pub use ash;

use ash::vk;

use instance::InstanceBundle;

#[derive(Debug)]
pub enum Error {
    Loading(ash::LoadingError),
    Vulkan(vk::Result),
    UnsupportedApiVersion(u32),
    NoPhysicalDevice,
    NoGraphicsQueue,
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
            Self::UnsupportedApiVersion(version) => {
                let major = vk::api_version_major(*version);
                let minor = vk::api_version_minor(*version);
                write!(f, "Vulkan 1.3 required, found {major}.{minor}")
            }
            Self::NoPhysicalDevice => write!(f, "no suitable physical device found"),
            Self::NoGraphicsQueue => write!(f, "no graphics queue family found"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Loading(err) => Some(err),
            Self::Vulkan(err) => Some(err),
            Self::UnsupportedApiVersion(_) | Self::NoPhysicalDevice | Self::NoGraphicsQueue => None,
        }
    }
}

pub(crate) fn supports_vulkan_1_3(version: u32) -> bool {
    let major = vk::api_version_major(version);
    let minor = vk::api_version_minor(version);
    major > 1 || (major == 1 && minor >= 3)
}

pub struct Context {
    // Declared first so the device is destroyed while the instance and its Entry are still alive.
    device: device::DeviceBundle,
    _instance: InstanceBundle,
}

impl Context {
    pub fn new() -> Result<Self, Error> {
        let instance = instance::create()?;
        let device = device::create(&instance.instance)?;
        Ok(Self {
            device,
            _instance: instance,
        })
    }

    pub fn physical_device(&self) -> vk::PhysicalDevice {
        self.device.physical_device
    }

    pub fn graphics_queue_family(&self) -> u32 {
        self.device.graphics_queue_family
    }

    pub fn graphics_queue(&self) -> vk::Queue {
        self.device.graphics_queue
    }
}
