use ash::vk;

const LOOK_SENSITIVITY: f32 = 0.002;
const DEFAULT_SPEED: f32 = 2.0;
const MIN_SPEED: f32 = 0.2;
const MAX_SPEED: f32 = 40.0;
const MAX_PITCH: f32 = 89_f32.to_radians();

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    position: glam::Vec3,
    yaw: f32,
    pitch: f32,
    speed: f32,
}

impl Camera {
    pub fn new() -> Self {
        let position = glam::Vec3::new(2.0, 1.5, 2.0);
        let forward = -position.normalize();
        Self {
            position,
            yaw: forward.x.atan2(-forward.z),
            pitch: forward.y.asin(),
            speed: DEFAULT_SPEED,
        }
    }

    /// `wish` is `(strafe, vertical, forward)` before normalization.
    /// `look_px` is mouse pixels: `+x` turns right, `+y` looks down.
    /// `boost` multiplies speed (shift).
    pub fn fly(&mut self, wish: [f32; 3], look_px: [f32; 2], dt: f32, boost: f32) {
        self.yaw += look_px[0] * LOOK_SENSITIVITY;
        self.pitch = (self.pitch - look_px[1] * LOOK_SENSITIVITY).clamp(-MAX_PITCH, MAX_PITCH);

        let forward = glam::Vec3::new(self.yaw.sin(), 0.0, -self.yaw.cos());
        let right = glam::Vec3::new(self.yaw.cos(), 0.0, self.yaw.sin());
        let mut direction = right * wish[0] + glam::Vec3::Y * wish[1] + forward * wish[2];
        if direction.length_squared() > 0.0 {
            direction = direction.normalize();
            self.position += direction * self.speed * boost * dt;
        }
    }

    pub fn position(&self) -> glam::Vec3 {
        self.position
    }

    pub fn yaw(&self) -> f32 {
        self.yaw
    }

    pub fn pitch(&self) -> f32 {
        self.pitch
    }

    pub fn speed(&self) -> f32 {
        self.speed
    }

    pub fn scroll_speed(&mut self, lines: f32) {
        self.speed = (self.speed * 1.1_f32.powf(lines)).clamp(MIN_SPEED, MAX_SPEED);
    }

    pub fn view(&self) -> glam::Mat4 {
        glam::Mat4::look_to_rh(self.position, self.look_direction(), glam::Vec3::Y)
    }

    /// Right-handed projection with Y up. The Vulkan scene pass flips Y itself.
    pub fn projection(&self, aspect: f32) -> glam::Mat4 {
        let aspect = if aspect.is_finite() && aspect > 0.0 {
            aspect
        } else {
            1.0
        };
        glam::Mat4::perspective_rh(60_f32.to_radians(), aspect, 0.1, 100.0)
    }

    pub fn view_proj(&self, extent: vk::Extent2D) -> glam::Mat4 {
        let aspect = extent.width as f32 / extent.height.max(1) as f32;
        let mut proj = self.projection(aspect);
        proj.y_axis.y *= -1.0;
        proj * self.view()
    }

    fn look_direction(&self) -> glam::Vec3 {
        let cos_pitch = self.pitch.cos();
        glam::Vec3::new(
            self.yaw.sin() * cos_pitch,
            self.pitch.sin(),
            -self.yaw.cos() * cos_pitch,
        )
    }
}

impl Default for Camera {
    fn default() -> Self {
        Self::new()
    }
}
