use ash::vk;

pub fn mvp(extent: vk::Extent2D) -> glam::Mat4 {
    let aspect = extent.width as f32 / extent.height as f32;
    let mut proj = glam::Mat4::perspective_rh(60_f32.to_radians(), aspect, 0.1, 100.0);
    proj.y_axis.y *= -1.0;
    let view = glam::Mat4::look_at_rh(
        glam::Vec3::new(2.0, 1.5, 2.0),
        glam::Vec3::ZERO,
        glam::Vec3::Y,
    );
    proj * view
}
