use ash::{Device, Instance, vk};

use crate::Error;
use crate::gpu::GpuImage;

pub struct ViewportTarget {
    image: GpuImage,
    pub extent: vk::Extent2D,
    pub format: vk::Format,
    pub layout: vk::ImageLayout,
}

impl ViewportTarget {
    pub fn create(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        extent: vk::Extent2D,
        format: vk::Format,
    ) -> Result<Self, Error> {
        let image = GpuImage::color_target(instance, physical_device, device, extent, format)?;
        Ok(Self {
            image,
            extent,
            format,
            layout: vk::ImageLayout::UNDEFINED,
        })
    }

    pub fn image(&self) -> vk::Image {
        self.image.image()
    }

    pub fn view(&self) -> vk::ImageView {
        self.image.view
    }
}
