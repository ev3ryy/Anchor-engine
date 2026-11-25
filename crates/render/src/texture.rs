use ash::{Device, Instance, vk};

use crate::Error;
use crate::gpu::{GpuBuffer, GpuImage};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TextureId {
    pub index: u32,
    pub generation: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum SamplerId {
    LinearRepeat = 0,
    LinearClamp = 1,
    NearestRepeat = 2,
    NearestClamp = 3,
}

struct TextureSlot {
    generation: u32,
    image: Option<GpuImage>,
}

struct RetiredTexture {
    index: u32,
    image: GpuImage,
}

struct PendingTexture {
    index: u32,
    generation: u32,
    extent: vk::Extent2D,
    image: GpuImage,
}

pub struct TextureStore {
    device: Device,
    instance: Instance,
    physical_device: vk::PhysicalDevice,
    staging: Option<GpuBuffer>,
    textures: Vec<TextureSlot>,
    free_textures: Vec<u32>,
    retired: Vec<RetiredTexture>,
    pending: Option<PendingTexture>,
}

impl TextureStore {
    pub fn new(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        capacity: u32,
    ) -> Self {
        Self {
            device: device.clone(),
            instance: instance.clone(),
            physical_device,
            staging: None,
            textures: (0..capacity)
                .map(|_| TextureSlot {
                    generation: 0,
                    image: None,
                })
                .collect(),
            free_textures: (1..capacity).rev().collect(),
            retired: Vec::new(),
            pending: None,
        }
    }

    pub fn prepare_white(&mut self) -> Result<(), Error> {
        self.prepare_image(0, 1, 1, &[255, 255, 255, 255])
    }

    pub fn prepare(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<(), Error> {
        let index = self.free_textures.pop().ok_or(Error::OutOfTextures)?;
        if let Err(err) = self.prepare_image(index, width, height, pixels) {
            self.free_textures.push(index);
            return Err(err);
        }
        Ok(())
    }

    pub fn upload_source(&self) -> (vk::Buffer, vk::Image, vk::Extent2D) {
        let pending = self.pending.as_ref().expect("texture upload");
        let staging = self.staging.as_ref().expect("staging buffer");
        (staging.buffer, pending.image.image(), pending.extent)
    }

    pub fn cancel_upload(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if pending.index != 0 {
            self.free_textures.push(pending.index);
        }
    }

    pub fn finish_upload(
        &mut self,
        write_sampled: impl FnOnce(u32, vk::ImageView) -> Result<(), Error>,
    ) -> Result<TextureId, Error> {
        let pending = self.pending.take().expect("texture upload");
        if let Err(err) = write_sampled(pending.index, pending.image.view) {
            if pending.index != 0 {
                self.free_textures.push(pending.index);
            }
            return Err(err);
        }
        let generation = pending.generation;
        let index = pending.index;
        let slot = &mut self.textures[index as usize];
        slot.generation = generation;
        slot.image = Some(pending.image);
        Ok(TextureId { index, generation })
    }

    pub fn free(&mut self, id: TextureId) -> Result<(), Error> {
        if id.index == 0 {
            return Err(Error::StaleTexture);
        }
        let slot = self
            .textures
            .get_mut(id.index as usize)
            .ok_or(Error::StaleTexture)?;
        if slot.generation != id.generation || slot.image.is_none() {
            return Err(Error::StaleTexture);
        }
        let image = slot.image.take().expect("live texture");
        slot.generation = next_generation(slot.generation);
        self.retired.push(RetiredTexture {
            index: id.index,
            image,
        });
        Ok(())
    }

    pub fn index(&self, id: TextureId) -> Result<u32, Error> {
        let slot = self
            .textures
            .get(id.index as usize)
            .ok_or(Error::StaleTexture)?;
        if slot.generation != id.generation || slot.image.is_none() {
            return Err(Error::StaleTexture);
        }
        Ok(id.index)
    }

    pub fn white_view(&self) -> vk::ImageView {
        self.textures[0].image.as_ref().expect("white texture").view
    }

    pub fn flush_retired(&mut self) -> Vec<(u32, GpuImage)> {
        std::mem::take(&mut self.retired)
            .into_iter()
            .map(|retired| (retired.index, retired.image))
            .collect()
    }

    pub fn reclaim(&mut self, index: u32, image: GpuImage) {
        drop(image);
        self.free_textures.push(index);
    }

    pub fn defer_retired(&mut self, index: u32, image: GpuImage) {
        self.retired.push(RetiredTexture { index, image });
    }

    fn prepare_image(
        &mut self,
        index: u32,
        width: u32,
        height: u32,
        pixels: &[u8],
    ) -> Result<(), Error> {
        let byte_count = (width as usize)
            .checked_mul(height as usize)
            .and_then(|texels| texels.checked_mul(4))
            .ok_or(Error::InvalidImage)?;
        if width == 0 || height == 0 || pixels.len() != byte_count {
            return Err(Error::InvalidImage);
        }
        self.ensure_staging(byte_count)?;
        let staging = self.staging.as_ref().expect("staging buffer");
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), staging.mapped(), pixels.len());
        }
        let generation = next_generation(self.textures[index as usize].generation);
        let image = GpuImage::sampled(
            &self.instance,
            self.physical_device,
            &self.device,
            vk::Extent2D { width, height },
            vk::Format::R8G8B8A8_SRGB,
        )?;
        self.pending = Some(PendingTexture {
            index,
            generation,
            extent: vk::Extent2D { width, height },
            image,
        });
        Ok(())
    }

    fn ensure_staging(&mut self, bytes: usize) -> Result<(), Error> {
        let size = bytes as vk::DeviceSize;
        if self
            .staging
            .as_ref()
            .is_some_and(|buffer| buffer.size() >= size)
        {
            return Ok(());
        }
        self.staging = Some(GpuBuffer::host_visible(
            &self.instance,
            self.physical_device,
            &self.device,
            size.max(4),
            vk::BufferUsageFlags::TRANSFER_SRC,
        )?);
        Ok(())
    }
}

fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 { 1 } else { next }
}
