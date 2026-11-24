use ash::{Device, vk};

use crate::Error;

pub struct CommandBundle {
    device: Device,
    pool: vk::CommandPool,
    buffer: vk::CommandBuffer,
}

impl Drop for CommandBundle {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_command_pool(self.pool, None);
        }
    }
}

pub fn create(device: &Device, family: u32) -> Result<CommandBundle, Error> {
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
        .queue_family_index(family);
    let pool = unsafe { device.create_command_pool(&pool_info, None)? };

    let alloc_info = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    let buffers = match unsafe { device.allocate_command_buffers(&alloc_info) } {
        Ok(buffers) => buffers,
        Err(err) => {
            unsafe { device.destroy_command_pool(pool, None) };
            return Err(err.into());
        }
    };

    Ok(CommandBundle {
        device: device.clone(),
        pool,
        buffer: buffers[0],
    })
}

impl CommandBundle {
    pub fn submit(
        &self,
        queue: vk::Queue,
        record: impl FnOnce(&Device, vk::CommandBuffer) -> Result<(), Error>,
    ) -> Result<(), Error> {
        unsafe {
            self.device
                .reset_command_buffer(self.buffer, vk::CommandBufferResetFlags::empty())?;
            let begin_info = vk::CommandBufferBeginInfo::default()
                .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
            self.device.begin_command_buffer(self.buffer, &begin_info)?;
        }

        if let Err(err) = record(&self.device, self.buffer) {
            unsafe {
                let _ = self
                    .device
                    .reset_command_buffer(self.buffer, vk::CommandBufferResetFlags::empty());
            }
            return Err(err);
        }

        unsafe {
            self.device.end_command_buffer(self.buffer)?;
            let submit_info =
                vk::SubmitInfo::default().command_buffers(std::slice::from_ref(&self.buffer));
            self.device.queue_submit(
                queue,
                std::slice::from_ref(&submit_info),
                vk::Fence::null(),
            )?;
            self.device.queue_wait_idle(queue)?;
        }
        Ok(())
    }

    pub fn record(
        &self,
        record: impl FnOnce(&Device, vk::CommandBuffer) -> Result<(), Error>,
    ) -> Result<(), Error> {
        let begin_info = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        unsafe { self.device.begin_command_buffer(self.buffer, &begin_info)? };

        if let Err(err) = record(&self.device, self.buffer) {
            unsafe {
                let _ = self
                    .device
                    .reset_command_buffer(self.buffer, vk::CommandBufferResetFlags::empty());
            }
            return Err(err);
        }

        unsafe { self.device.end_command_buffer(self.buffer)? };
        Ok(())
    }

    pub fn submit_frame(
        &self,
        queue: vk::Queue,
        wait: vk::Semaphore,
        signal: vk::Semaphore,
        fence: vk::Fence,
    ) -> Result<(), Error> {
        let wait_info = vk::SemaphoreSubmitInfo::default()
            .semaphore(wait)
            .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT);
        let signal_info = vk::SemaphoreSubmitInfo::default()
            .semaphore(signal)
            .stage_mask(vk::PipelineStageFlags2::COLOR_ATTACHMENT_OUTPUT);
        let command_info = vk::CommandBufferSubmitInfo::default().command_buffer(self.buffer);
        let submit = vk::SubmitInfo2::default()
            .wait_semaphore_infos(std::slice::from_ref(&wait_info))
            .command_buffer_infos(std::slice::from_ref(&command_info))
            .signal_semaphore_infos(std::slice::from_ref(&signal_info));
        unsafe {
            self.device
                .queue_submit2(queue, std::slice::from_ref(&submit), fence)?;
        }
        Ok(())
    }
}
