use egui_ash_renderer::vulkan::{
    create_vulkan_descriptor_set, create_vulkan_descriptor_set_layout,
};
use render::ash::{Device, vk};

pub struct SceneTexture {
    sampler: vk::Sampler,
    pool: vk::DescriptorPool,
    layout: vk::DescriptorSetLayout,
    set: vk::DescriptorSet,
    view: vk::ImageView,
    pub id: egui::TextureId,
}

impl SceneTexture {
    pub fn create(
        device: &Device,
        renderer: &mut crate::GpuRenderer,
        view: vk::ImageView,
    ) -> Result<Self, render::Error> {
        let sampler = match unsafe { device.create_sampler(&sampler_info(), None) } {
            Ok(sampler) => sampler,
            Err(err) => return Err(err.into()),
        };
        let layout = match create_vulkan_descriptor_set_layout(device) {
            Ok(layout) => layout,
            Err(err) => {
                unsafe { device.destroy_sampler(sampler, None) };
                return Err(overlay(err));
            }
        };
        let pool_sizes = [vk::DescriptorPoolSize {
            ty: vk::DescriptorType::COMBINED_IMAGE_SAMPLER,
            descriptor_count: 1,
        }];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(&pool_sizes)
            .max_sets(1);
        let pool = match unsafe { device.create_descriptor_pool(&pool_info, None) } {
            Ok(pool) => pool,
            Err(err) => {
                unsafe {
                    device.destroy_descriptor_set_layout(layout, None);
                    device.destroy_sampler(sampler, None);
                }
                return Err(err.into());
            }
        };
        let set = match create_vulkan_descriptor_set(device, layout, pool, view, sampler) {
            Ok(set) => set,
            Err(err) => {
                unsafe {
                    device.destroy_descriptor_pool(pool, None);
                    device.destroy_descriptor_set_layout(layout, None);
                    device.destroy_sampler(sampler, None);
                }
                return Err(overlay(err));
            }
        };
        let id = renderer.add_user_texture(set);
        Ok(Self {
            sampler,
            pool,
            layout,
            set,
            view,
            id,
        })
    }

    pub fn sync(&mut self, device: &Device, view: vk::ImageView) {
        if self.view == view {
            return;
        }
        self.view = view;
        let image_info = [vk::DescriptorImageInfo {
            sampler: self.sampler,
            image_view: view,
            image_layout: vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        }];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(self.set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe { device.update_descriptor_sets(&writes, &[]) };
    }

    pub fn destroy(self, device: &Device) {
        unsafe {
            device.destroy_descriptor_pool(self.pool, None);
            device.destroy_descriptor_set_layout(self.layout, None);
            device.destroy_sampler(self.sampler, None);
        }
    }
}

fn sampler_info() -> vk::SamplerCreateInfo<'static> {
    vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::LINEAR)
        .min_filter(vk::Filter::LINEAR)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .max_lod(0.0)
}

fn overlay(err: impl std::fmt::Display) -> render::Error {
    render::Error::Overlay(err.to_string())
}
