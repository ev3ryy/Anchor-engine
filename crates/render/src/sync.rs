use ash::{Device, vk};

use crate::Error;

pub struct FrameSync {
    device: Device,
    pub image_available: vk::Semaphore,
    pub in_flight: vk::Fence,
}

impl Drop for FrameSync {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_semaphore(self.image_available, None);
            self.device.destroy_fence(self.in_flight, None);
        }
    }
}

pub fn create(device: &Device) -> Result<FrameSync, Error> {
    let image_available =
        unsafe { device.create_semaphore(&vk::SemaphoreCreateInfo::default(), None)? };
    let fence_info = vk::FenceCreateInfo::default().flags(vk::FenceCreateFlags::SIGNALED);
    let in_flight = match unsafe { device.create_fence(&fence_info, None) } {
        Ok(fence) => fence,
        Err(err) => {
            unsafe { device.destroy_semaphore(image_available, None) };
            return Err(err.into());
        }
    };

    Ok(FrameSync {
        device: device.clone(),
        image_available,
        in_flight,
    })
}
