use ash::{Device, Instance, vk};

use crate::Error;
use crate::gpu::{GpuBuffer, GpuImage};
use crate::material::{
    HEADER_STRIDE, MATERIAL_CAPACITY, MaterialDesc, MaterialId, MaterialStore, PARAM_STRIDE,
    SamplerId, TextureId,
};
use crate::shader::layout::{FRAME_PUSH_BYTES, frame_push_stages};

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

pub struct Bindless {
    device: Device,
    instance: Instance,
    physical_device: vk::PhysicalDevice,
    set_layout: vk::DescriptorSetLayout,
    pipeline_layout: vk::PipelineLayout,
    pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    samplers: Vec<vk::Sampler>,
    staging: Option<GpuBuffer>,
    headers: GpuBuffer,
    params: GpuBuffer,
    textures: Vec<TextureSlot>,
    free_textures: Vec<u32>,
    retired: Vec<RetiredTexture>,
    pending: Option<PendingTexture>,
    materials: MaterialStore,
}

impl Drop for Bindless {
    fn drop(&mut self) {
        unsafe {
            self.device.destroy_descriptor_pool(self.pool, None);
            self.device
                .destroy_pipeline_layout(self.pipeline_layout, None);
            self.device
                .destroy_descriptor_set_layout(self.set_layout, None);
            for sampler in &self.samplers {
                self.device.destroy_sampler(*sampler, None);
            }
        }
    }
}

impl Bindless {
    pub fn allocate(
        instance: &Instance,
        physical_device: vk::PhysicalDevice,
        device: &Device,
        texture_capacity: u32,
    ) -> Result<Self, Error> {
        let set_layout = create_set_layout(device, texture_capacity)?;
        let pipeline_layout = match create_pipeline_layout(device, set_layout) {
            Ok(layout) => layout,
            Err(err) => {
                unsafe { device.destroy_descriptor_set_layout(set_layout, None) };
                return Err(err);
            }
        };
        let pool = match create_pool(device, texture_capacity) {
            Ok(pool) => pool,
            Err(err) => {
                unsafe {
                    device.destroy_pipeline_layout(pipeline_layout, None);
                    device.destroy_descriptor_set_layout(set_layout, None);
                }
                return Err(err);
            }
        };
        let set = match allocate_set(device, pool, set_layout) {
            Ok(set) => set,
            Err(err) => {
                unsafe {
                    device.destroy_descriptor_pool(pool, None);
                    device.destroy_pipeline_layout(pipeline_layout, None);
                    device.destroy_descriptor_set_layout(set_layout, None);
                }
                return Err(err);
            }
        };

        let header_bytes = MATERIAL_CAPACITY as vk::DeviceSize * HEADER_STRIDE as vk::DeviceSize;
        let param_bytes = MATERIAL_CAPACITY as vk::DeviceSize * PARAM_STRIDE as vk::DeviceSize;
        let headers = GpuBuffer::host_visible(
            instance,
            physical_device,
            device,
            header_bytes,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        );
        let headers = match headers {
            Ok(buffer) => buffer,
            Err(err) => {
                unsafe {
                    device.destroy_descriptor_pool(pool, None);
                    device.destroy_pipeline_layout(pipeline_layout, None);
                    device.destroy_descriptor_set_layout(set_layout, None);
                }
                return Err(err);
            }
        };
        let params = match GpuBuffer::host_visible(
            instance,
            physical_device,
            device,
            param_bytes,
            vk::BufferUsageFlags::STORAGE_BUFFER,
        ) {
            Ok(buffer) => buffer,
            Err(err) => {
                drop(headers);
                unsafe {
                    device.destroy_descriptor_pool(pool, None);
                    device.destroy_pipeline_layout(pipeline_layout, None);
                    device.destroy_descriptor_set_layout(set_layout, None);
                }
                return Err(err);
            }
        };

        let mut heap = Self {
            device: device.clone(),
            instance: instance.clone(),
            physical_device,
            set_layout,
            pipeline_layout,
            pool,
            set,
            samplers: Vec::new(),
            staging: None,
            headers,
            params,
            textures: (0..texture_capacity)
                .map(|_| TextureSlot {
                    generation: 0,
                    image: None,
                })
                .collect(),
            free_textures: (1..texture_capacity).rev().collect(),
            retired: Vec::new(),
            pending: None,
            materials: MaterialStore::new(MATERIAL_CAPACITY),
        };
        let header = heap.headers.buffer;
        let params = heap.params.buffer;
        heap.write_buffer(2, header)?;
        heap.write_buffer(3, params)?;
        heap.create_samplers()?;
        Ok(heap)
    }

    pub fn prepare_white(&mut self) -> Result<(), Error> {
        self.prepare_image(0, 1, 1, &[255, 255, 255, 255])
    }

    pub fn pipeline_layout(&self) -> vk::PipelineLayout {
        self.pipeline_layout
    }

    pub fn descriptor_set(&self) -> vk::DescriptorSet {
        self.set
    }

    pub fn material_buffers(&self) -> [vk::Buffer; 2] {
        [self.headers.buffer, self.params.buffer]
    }

    pub fn prepare_texture(&mut self, width: u32, height: u32, pixels: &[u8]) -> Result<(), Error> {
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

    pub fn finish_upload(&mut self) -> Result<TextureId, Error> {
        let pending = self.pending.take().expect("texture upload");
        if let Err(err) = self.write_sampled(pending.index, pending.image.view) {
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

    pub fn free_texture(&mut self, id: TextureId) -> Result<(), Error> {
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

    pub fn create_material(&mut self, desc: MaterialDesc) -> Result<MaterialId, Error> {
        let (albedo, sampler, color) = self.resolve(desc)?;
        self.materials.create(albedo, sampler, color)
    }

    pub fn update_material(&mut self, id: MaterialId, desc: MaterialDesc) -> Result<(), Error> {
        let (albedo, sampler, color) = self.resolve(desc)?;
        self.materials.update(id, albedo, sampler, color)
    }

    pub fn free_material(&mut self, id: MaterialId) -> Result<(), Error> {
        self.materials.free(id)
    }

    pub fn material_index(&self, id: MaterialId) -> Result<u32, Error> {
        self.materials.index(id)
    }

    pub fn flush(&mut self) {
        self.materials
            .flush(self.headers.mapped(), self.params.mapped());
        let white_view = self.textures[0].image.as_ref().expect("white texture").view;
        let retired = std::mem::take(&mut self.retired);
        for retired in retired {
            if self.write_sampled(retired.index, white_view).is_err() {
                self.retired.push(retired);
                continue;
            }
            let index = retired.index;
            drop(retired.image);
            self.free_textures.push(index);
        }
    }

    fn resolve(&self, desc: MaterialDesc) -> Result<(u32, SamplerId, [f32; 4]), Error> {
        match desc {
            MaterialDesc::Unlit {
                color,
                albedo,
                sampler,
            } => Ok((self.texture_index(albedo)?, sampler, color)),
        }
    }

    fn texture_index(&self, id: TextureId) -> Result<u32, Error> {
        let slot = self
            .textures
            .get(id.index as usize)
            .ok_or(Error::StaleTexture)?;
        if slot.generation != id.generation || slot.image.is_none() {
            return Err(Error::StaleTexture);
        }
        Ok(id.index)
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

    fn create_samplers(&mut self) -> Result<(), Error> {
        let specs = [
            (vk::Filter::LINEAR, vk::SamplerAddressMode::REPEAT),
            (vk::Filter::LINEAR, vk::SamplerAddressMode::CLAMP_TO_EDGE),
            (vk::Filter::NEAREST, vk::SamplerAddressMode::REPEAT),
            (vk::Filter::NEAREST, vk::SamplerAddressMode::CLAMP_TO_EDGE),
        ];
        for (index, (filter, address)) in specs.into_iter().enumerate() {
            let sampler = create_sampler(&self.device, filter, address)?;
            self.write_sampler(index as u32, sampler);
            self.samplers.push(sampler);
        }
        Ok(())
    }

    fn write_sampler(&self, index: u32, sampler: vk::Sampler) {
        let info = vk::DescriptorImageInfo::default().sampler(sampler);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(1)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .image_info(std::slice::from_ref(&info));
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
    }

    fn write_sampled(&self, index: u32, view: vk::ImageView) -> Result<(), Error> {
        let info = vk::DescriptorImageInfo::default()
            .image_view(view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(0)
            .dst_array_element(index)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .image_info(std::slice::from_ref(&info));
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Ok(())
    }

    fn write_buffer(&self, binding: u32, buffer: vk::Buffer) -> Result<(), Error> {
        let info = vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .offset(0)
            .range(vk::WHOLE_SIZE);
        let write = vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(binding)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(std::slice::from_ref(&info));
        unsafe { self.device.update_descriptor_sets(&[write], &[]) };
        Ok(())
    }
}

fn next_generation(generation: u32) -> u32 {
    let next = generation.wrapping_add(1);
    if next == 0 { 1 } else { next }
}

fn create_set_layout(
    device: &Device,
    texture_capacity: u32,
) -> Result<vk::DescriptorSetLayout, Error> {
    let stages = frame_push_stages();
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
            .descriptor_count(texture_capacity)
            .stage_flags(stages),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::SAMPLER)
            .descriptor_count(4)
            .stage_flags(stages),
        vk::DescriptorSetLayoutBinding::default()
            .binding(2)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(stages),
        vk::DescriptorSetLayoutBinding::default()
            .binding(3)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(stages),
    ];
    let flags = [
        vk::DescriptorBindingFlags::PARTIALLY_BOUND | vk::DescriptorBindingFlags::UPDATE_AFTER_BIND,
        vk::DescriptorBindingFlags::empty(),
        vk::DescriptorBindingFlags::empty(),
        vk::DescriptorBindingFlags::empty(),
    ];
    let mut binding_flags =
        vk::DescriptorSetLayoutBindingFlagsCreateInfo::default().binding_flags(&flags);
    let info = vk::DescriptorSetLayoutCreateInfo::default()
        .flags(vk::DescriptorSetLayoutCreateFlags::UPDATE_AFTER_BIND_POOL)
        .bindings(&bindings)
        .push_next(&mut binding_flags);
    unsafe {
        device
            .create_descriptor_set_layout(&info, None)
            .map_err(Error::from)
    }
}

fn create_pipeline_layout(
    device: &Device,
    set_layout: vk::DescriptorSetLayout,
) -> Result<vk::PipelineLayout, Error> {
    let push = vk::PushConstantRange::default()
        .stage_flags(frame_push_stages())
        .offset(0)
        .size(FRAME_PUSH_BYTES);
    let info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(std::slice::from_ref(&set_layout))
        .push_constant_ranges(std::slice::from_ref(&push));
    unsafe {
        device
            .create_pipeline_layout(&info, None)
            .map_err(Error::from)
    }
}

fn create_pool(device: &Device, texture_capacity: u32) -> Result<vk::DescriptorPool, Error> {
    let sizes = [
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::SAMPLED_IMAGE,
            descriptor_count: texture_capacity,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::SAMPLER,
            descriptor_count: 4,
        },
        vk::DescriptorPoolSize {
            ty: vk::DescriptorType::STORAGE_BUFFER,
            descriptor_count: 2,
        },
    ];
    let info = vk::DescriptorPoolCreateInfo::default()
        .flags(vk::DescriptorPoolCreateFlags::UPDATE_AFTER_BIND)
        .max_sets(1)
        .pool_sizes(&sizes);
    unsafe {
        device
            .create_descriptor_pool(&info, None)
            .map_err(Error::from)
    }
}

fn allocate_set(
    device: &Device,
    pool: vk::DescriptorPool,
    layout: vk::DescriptorSetLayout,
) -> Result<vk::DescriptorSet, Error> {
    let info = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(std::slice::from_ref(&layout));
    let sets = unsafe { device.allocate_descriptor_sets(&info)? };
    Ok(sets[0])
}

fn create_sampler(
    device: &Device,
    filter: vk::Filter,
    address: vk::SamplerAddressMode,
) -> Result<vk::Sampler, Error> {
    let mipmap = if filter == vk::Filter::LINEAR {
        vk::SamplerMipmapMode::LINEAR
    } else {
        vk::SamplerMipmapMode::NEAREST
    };
    let info = vk::SamplerCreateInfo::default()
        .mag_filter(filter)
        .min_filter(filter)
        .mipmap_mode(mipmap)
        .address_mode_u(address)
        .address_mode_v(address)
        .address_mode_w(address)
        .min_lod(0.0)
        .max_lod(0.0);
    unsafe { device.create_sampler(&info, None).map_err(Error::from) }
}
