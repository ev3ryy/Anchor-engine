mod panels;
mod scene;

use egui_dock::DockState;
use panels::{Panels, Tab};
use render::ash::vk;
use window::winit::window::Window;

type GpuRenderer = egui_ash_renderer::Renderer<egui_ash_renderer::allocator::DefaultAllocator>;

pub struct FrameView<'a> {
    pub camera: &'a render::Camera,
    pub dt: f32,
    pub extent: [u32; 2],
}

#[derive(Debug)]
pub enum Error {
    Vulkan(vk::Result),
    Renderer(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vulkan(err) => write!(f, "Vulkan error: {err}"),
            Self::Renderer(message) => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

pub struct Editor {
    // Drop fields after `Drop::drop`: the renderer while `device` is alive, then the pool handle.
    renderer: GpuRenderer,
    context: egui::Context,
    winit: egui_winit::State,
    dock: Option<DockState<Tab>>,
    primitives: Vec<egui::ClippedPrimitive>,
    pending_set: Vec<(egui::TextureId, egui::epaint::ImageDelta)>,
    pending_free: Vec<egui::TextureId>,
    free_ready: Vec<egui::TextureId>,
    viewport_rect: Option<egui::Rect>,
    scene: Option<scene::SceneTexture>,
    pixels_per_point: f32,
    color_format: vk::Format,
    repaint: bool,
    device: render::ash::Device,
    pool: vk::CommandPool,
}

impl Editor {
    pub fn new(render: &mut render::Context, window: &Window) -> Result<Self, Error> {
        let device = render.device().clone();
        let pool = unsafe {
            device.create_command_pool(
                &vk::CommandPoolCreateInfo::default()
                    .flags(vk::CommandPoolCreateFlags::RESET_COMMAND_BUFFER)
                    .queue_family_index(render.graphics_queue_family()),
                None,
            )
        }
        .map_err(Error::Vulkan)?;

        let color_format = render.color_format();
        let renderer = match GpuRenderer::with_default_allocator(
            render.instance(),
            render.physical_device(),
            device.clone(),
            dynamic_rendering(color_format),
            egui_ash_renderer::Options {
                in_flight_frames: 1,
                srgb_framebuffer: is_srgb(color_format),
                ..Default::default()
            },
        ) {
            Ok(renderer) => renderer,
            Err(err) => {
                unsafe { device.destroy_command_pool(pool, None) };
                return Err(Error::Renderer(err.to_string()));
            }
        };

        let context = egui::Context::default();
        let winit = egui_winit::State::new(
            context.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let window_size = window.inner_size();
        let scene_view =
            match render.scene_target(window_size.width.max(1), window_size.height.max(1)) {
                Ok(view) => view,
                Err(err) => {
                    drop(renderer);
                    unsafe { device.destroy_command_pool(pool, None) };
                    return Err(from_render(err));
                }
            };

        let mut editor = Self {
            renderer,
            context,
            winit,
            dock: Some(panels::dock_state()),
            primitives: Vec::new(),
            pending_set: Vec::new(),
            pending_free: Vec::new(),
            free_ready: Vec::new(),
            viewport_rect: None,
            scene: None,
            pixels_per_point: window.scale_factor() as f32,
            color_format,
            repaint: false,
            device,
            pool,
        };
        if let Err(err) = editor.bind_scene(scene_view) {
            return Err(from_render(err));
        }
        Ok(editor)
    }

    pub fn on_window_event(
        &mut self,
        window: &Window,
        event: &window::winit::event::WindowEvent,
    ) -> bool {
        self.winit.on_window_event(window, event).repaint
    }

    pub fn begin_frame(&mut self, window: &Window, view: FrameView<'_>) {
        let mut dock = self.dock.take().unwrap_or_else(panels::dock_state);
        let mut viewport_rect = None;
        let mut viewport_px = self.viewport_pixels().unwrap_or([0, 0]);
        let scene = self.scene.as_ref().map(|scene| scene.id);
        let raw_input = self.winit.take_egui_input(window);
        let mut output = self.context.run_ui(raw_input, |ui| {
            let mut panels = Panels {
                viewport: &mut viewport_rect,
                viewport_px: &mut viewport_px,
                scene,
                camera: view.camera,
                dt: view.dt,
                extent: view.extent,
            };
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    egui_dock::DockArea::new(&mut dock).show_inside(ui, &mut panels);
                });
        });
        self.dock = Some(dock);
        self.viewport_rect = viewport_rect;
        self.pixels_per_point = output.pixels_per_point;
        self.repaint = output
            .viewport_output
            .values()
            .any(|viewport| viewport.repaint_delay.is_zero());
        let set = std::mem::take(&mut output.textures_delta.set);
        let free = std::mem::take(&mut output.textures_delta.free);
        self.pending_set.extend(
            set.into_iter()
                .flat_map(|(id, deltas)| deltas.into_iter().map(move |delta| (id, delta))),
        );
        self.pending_free.extend(free);
        self.primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        self.winit
            .handle_platform_output(window, output.platform_output);
    }

    /// Uploads `set` now and frees the previous frame's ids. This frame's frees wait until the
    /// next call, which runs only after that frame's fence has been waited.
    pub fn prepare(&mut self, queue: vk::Queue) -> Result<(), render::Error> {
        while let Some(id) = self.free_ready.first().copied() {
            self.renderer.free_texture(id).map_err(overlay_error)?;
            self.free_ready.remove(0);
        }

        let pending = std::mem::take(&mut self.pending_set);
        let mut rest = pending.into_iter();
        while let Some((id, delta)) = rest.next() {
            if let Err(err) = self.renderer.set_texture(queue, self.pool, id, &delta) {
                self.pending_set.push((id, delta));
                self.pending_set.extend(rest);
                return Err(overlay_error(err));
            }
        }

        self.free_ready.append(&mut self.pending_free);
        Ok(())
    }

    pub fn paint(&mut self, overlay: render::Overlay<'_>) -> Result<(), render::Error> {
        if let Some(view) = overlay.scene_view {
            self.bind_scene(view)?;
        }
        let color = vk::RenderingAttachmentInfo::default()
            .image_view(overlay.color_view)
            .image_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .load_op(overlay.load_op)
            .store_op(vk::AttachmentStoreOp::STORE)
            .clear_value(vk::ClearValue {
                color: vk::ClearColorValue {
                    float32: [0.10, 0.10, 0.10, 1.0],
                },
            });
        let rendering = vk::RenderingInfo::default()
            .render_area(vk::Rect2D {
                offset: vk::Offset2D::default(),
                extent: overlay.extent,
            })
            .layer_count(1)
            .color_attachments(std::slice::from_ref(&color));
        unsafe {
            overlay
                .device
                .cmd_begin_rendering(overlay.command_buffer, &rendering);
        }
        let drawn = self.renderer.cmd_draw(
            overlay.command_buffer,
            overlay.extent,
            self.pixels_per_point,
            &self.primitives,
        );
        unsafe {
            overlay.device.cmd_end_rendering(overlay.command_buffer);
        }
        drawn.map_err(overlay_error)
    }

    pub fn sync_format(&mut self, format: vk::Format) -> Result<(), render::Error> {
        if format == self.color_format {
            return Ok(());
        }
        self.color_format = format;
        self.renderer
            .set_render_mode(dynamic_rendering(format))
            .map_err(overlay_error)
    }

    pub fn wants_keyboard(&self) -> bool {
        self.context.egui_wants_keyboard_input()
    }

    pub fn egui_is_using_pointer(&self) -> bool {
        self.context.egui_is_using_pointer()
    }

    pub fn viewport_pixels(&self) -> Option<[u32; 2]> {
        let rect = self.viewport_rect?;
        let size = physical_pixels(rect, self.pixels_per_point);
        (size[0] > 0 && size[1] > 0).then_some(size)
    }

    pub fn pointer_in_viewport(&self, cursor_px: [f32; 2]) -> bool {
        let Some(rect) = self.viewport_rect else {
            return false;
        };
        let scale = self.pixels_per_point.max(f32::MIN_POSITIVE);
        rect.contains(egui::pos2(cursor_px[0] / scale, cursor_px[1] / scale))
    }

    pub fn wants_repaint(&self) -> bool {
        self.repaint
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        // The last frame can still reference the UI pipeline and font image.
        unsafe {
            let _ = self.device.device_wait_idle();
            if let Some(scene) = self.scene.take() {
                self.renderer.remove_user_texture(scene.id);
                scene.destroy(&self.device);
            }
            if self.pool != vk::CommandPool::null() {
                self.device.destroy_command_pool(self.pool, None);
                self.pool = vk::CommandPool::null();
            }
        }
    }
}

fn dynamic_rendering(format: vk::Format) -> egui_ash_renderer::RenderMode {
    egui_ash_renderer::RenderMode::DynamicRendering(egui_ash_renderer::DynamicRendering {
        color_attachment_format: format,
        depth_attachment_format: None,
        stencil_attachment_format: None,
    })
}

fn is_srgb(format: vk::Format) -> bool {
    matches!(
        format,
        vk::Format::R8G8B8A8_SRGB
            | vk::Format::B8G8R8A8_SRGB
            | vk::Format::A8B8G8R8_SRGB_PACK32
            | vk::Format::R8G8B8_SRGB
            | vk::Format::B8G8R8_SRGB
    )
}

impl Editor {
    fn bind_scene(&mut self, view: vk::ImageView) -> Result<(), render::Error> {
        if let Some(scene) = self.scene.as_mut() {
            scene.sync(&self.device, view);
            return Ok(());
        }
        self.scene = Some(scene::SceneTexture::create(
            &self.device,
            &mut self.renderer,
            view,
        )?);
        Ok(())
    }
}

pub(crate) fn physical_pixels(rect: egui::Rect, pixels_per_point: f32) -> [u32; 2] {
    let scale = pixels_per_point.max(f32::MIN_POSITIVE);
    [
        (rect.width() * scale).round() as u32,
        (rect.height() * scale).round() as u32,
    ]
}

fn from_render(err: render::Error) -> Error {
    match err {
        render::Error::Vulkan(result) => Error::Vulkan(result),
        other => Error::Renderer(other.to_string()),
    }
}

fn overlay_error(err: impl std::fmt::Display) -> render::Error {
    render::Error::Overlay(err.to_string())
}
