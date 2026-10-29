//! The interface kit: Angelica's look for a plugin's own surface. It runs
//! egui inside the plugin with Angelica's themes and fonts, as the player
//! set them, and draws through the GPU API on a surface that covers the
//! game's window, taking the mouse only where its windows are.
//!
//! ```ignore
//! struct Hello { kit: angelica_kit::Kit }
//!
//! impl angelica_sdk::Plugin for Hello {
//!     fn start() -> Result<Self, String> {
//!         Ok(Self { kit: angelica_kit::Kit::new()? })
//!     }
//!
//!     fn surface_input(&mut self, events: Vec<angelica_sdk::gpu::SurfaceEvent>) {
//!         self.kit.input(&events);
//!     }
//!
//!     fn tick(&mut self, _: f64) -> Result<(), String> {
//!         self.kit.frame(|context| {
//!             egui::Window::new("Hello").show(context, |ui| ui.label("A window of the plugin's own"));
//!         })?;
//!         Ok(())
//!     }
//! }
//! ```
//!
//! A plugin may draw anything else with egui's painter on the same frame,
//! such as marks at places of the world with [`angelica_sdk::gpu::project`].

pub use egui;

use angelica_sdk::Error;
use angelica_sdk::api::look;
use angelica_sdk::gpu::{
    self, AddressMode, Binding, Blend, BufferUsage, Command, DrawIndexed, Filter, IndexBuffer,
    Object, PipelineDesc, Rect, SurfaceEvent, TextureFormat, Topology, VertexAttribute,
    VertexBuffer, VertexFormat,
};
use std::collections::HashMap;

const SHADER: &str = r"
struct Screen { size: vec2<f32>, unused: vec2<f32> };
@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32>, @location(1) color: vec4<f32> };
@vertex fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> Out {
    var o: Out;
    o.pos = vec4<f32>(2.0 * pos.x / screen.size.x - 1.0, 1.0 - 2.0 * pos.y / screen.size.y, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}
@fragment fn fs(i: Out) -> @location(0) vec4<f32> {
    return i.color * textureSample(image, smp, i.uv);
}
";

/// Bytes of one vertex: position, texture coordinates, and color.
const STRIDE: u32 = 20;

/// A buffer that is made again larger when a frame needs more room.
struct Growing {
    usage: BufferUsage,
    buffer: Option<(Object, u32)>,
}

impl Growing {
    fn with_room(&mut self, bytes: u32) -> Result<Object, Error> {
        if let Some((buffer, size)) = self.buffer
            && size >= bytes
        {
            return Ok(buffer);
        }
        if let Some((old, _)) = self.buffer.take() {
            gpu::destroy(old);
        }
        let size = bytes.next_power_of_two().max(64 * 1024);
        let buffer = gpu::create_buffer(size, self.usage)?;
        self.buffer = Some((buffer, size));
        Ok(buffer)
    }
}

/// A texture of egui's and the bind group that draws with it.
struct Texture {
    image: Object,
    group: Object,
    size: [usize; 2],
}

/// egui on a surface of the plugin's that covers the game's window.
pub struct Kit {
    context: egui::Context,
    surface: Object,
    area: Rect,
    pipeline: Object,
    uniform: Object,
    sampler: Object,
    vertices: Growing,
    indices: Growing,
    textures: HashMap<egui::TextureId, Texture>,
    events: Vec<egui::Event>,
    started: std::time::Instant,
    scale: f32,
}

fn rect_of(rect: egui::Rect, scale: f32) -> Rect {
    let to = |value: f32| num_cast(value * scale);
    Rect {
        x: to(rect.min.x),
        y: to(rect.min.y),
        width: u32::try_from(to(rect.width()).max(0)).unwrap_or(0),
        height: u32::try_from(to(rect.height()).max(0)).unwrap_or(0),
    }
}

#[allow(clippy::cast_possible_truncation)]
fn num_cast(value: f32) -> i32 {
    if value.is_finite() {
        value.round().clamp(-1.0e9, 1.0e9) as i32
    } else {
        0
    }
}

fn whole_screen() -> Rect {
    let size = gpu::screen();
    Rect {
        x: 0,
        y: 0,
        width: size.width.max(1),
        height: size.height.max(1),
    }
}

impl Kit {
    /// A kit on a new surface over the game's window, with Angelica's
    /// fonts and the player's theme.
    ///
    /// # Errors
    /// The GPU objects it needs cannot be made.
    pub fn new() -> Result<Self, Error> {
        let area = whole_screen();
        let surface = gpu::create_surface(area)?;
        let shader = gpu::create_shader(SHADER)?;
        let pipeline = gpu::create_pipeline(&PipelineDesc {
            shader,
            vertex_entry: "vs".into(),
            fragment_entry: "fs".into(),
            stride: STRIDE,
            attributes: vec![
                VertexAttribute {
                    location: 0,
                    format: VertexFormat::Float32x2,
                    offset: 0,
                },
                VertexAttribute {
                    location: 1,
                    format: VertexFormat::Float32x2,
                    offset: 8,
                },
                VertexAttribute {
                    location: 2,
                    format: VertexFormat::Unorm8x4,
                    offset: 16,
                },
            ],
            topology: Topology::TriangleList,
            blend: Blend::Premultiplied,
        })?;
        let context = egui::Context::default();
        let faces = angelica_look::fonts::FACE_NAMES
            .iter()
            .filter_map(|name| {
                look::font(name)
                    .map(|bytes| ((*name).to_owned(), egui::FontData::from_owned(bytes)))
            })
            .collect();
        context.set_fonts(angelica_look::fonts::definitions_of(faces, Vec::new()));
        let settings = look::settings();
        angelica_look::theme::apply(
            &context,
            if settings.dark {
                angelica_look::theme::Theme::Dark
            } else {
                angelica_look::theme::Theme::Light
            },
        );
        Ok(Self {
            context,
            surface,
            area,
            pipeline,
            uniform: gpu::create_buffer(16, BufferUsage::UNIFORM)?,
            sampler: gpu::create_sampler(Filter::Linear, AddressMode::Clamp)?,
            vertices: Growing {
                usage: BufferUsage::VERTEX,
                buffer: None,
            },
            indices: Growing {
                usage: BufferUsage::INDEX,
                buffer: None,
            },
            textures: HashMap::new(),
            events: Vec::new(),
            started: std::time::Instant::now(),
            scale: settings.scale.max(0.5),
        })
    }

    /// The surface the kit draws on.
    #[must_use]
    pub fn surface(&self) -> Object {
        self.surface
    }

    /// The egui context, for the plugin's own settings of it.
    #[must_use]
    pub fn context(&self) -> &egui::Context {
        &self.context
    }

    /// Takes the player's input of this frame on the kit's surface.
    pub fn input(&mut self, events: &[SurfaceEvent]) {
        let scale = self.scale;
        let at = |x: f32, y: f32| egui::pos2(x / scale, y / scale);
        let held = |modifiers: &gpu::Modifiers| egui::Modifiers {
            alt: modifiers.alt,
            ctrl: modifiers.ctrl,
            shift: modifiers.shift,
            mac_cmd: false,
            command: modifiers.ctrl,
        };
        let button = |button: gpu::Button| match button {
            gpu::Button::Left => egui::PointerButton::Primary,
            gpu::Button::Right => egui::PointerButton::Secondary,
            gpu::Button::Middle => egui::PointerButton::Middle,
        };
        for event in events {
            let ours = match event {
                SurfaceEvent::PointerMoved(pointer) => pointer.surface,
                SurfaceEvent::PointerLeft(surface) | SurfaceEvent::FocusLost(surface) => *surface,
                SurfaceEvent::Pressed(press) | SurfaceEvent::Released(press) => press.surface,
                SurfaceEvent::Wheel(wheel) => wheel.surface,
                SurfaceEvent::KeyPressed(key) | SurfaceEvent::KeyReleased(key) => key.surface,
                SurfaceEvent::Text(typed) => typed.surface,
            };
            if ours != self.surface {
                continue;
            }
            let event = match event {
                SurfaceEvent::PointerMoved(pointer) => {
                    egui::Event::PointerMoved(at(pointer.x, pointer.y))
                }
                SurfaceEvent::PointerLeft(_) => egui::Event::PointerGone,
                SurfaceEvent::Pressed(press) | SurfaceEvent::Released(press) => {
                    egui::Event::PointerButton {
                        pos: at(press.x, press.y),
                        button: button(press.button),
                        pressed: matches!(event, SurfaceEvent::Pressed(_)),
                        modifiers: held(&press.modifiers),
                    }
                }
                SurfaceEvent::Wheel(wheel) => egui::Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(wheel.x / scale, wheel.y / scale),
                    phase: egui::TouchPhase::Move,
                    modifiers: egui::Modifiers::NONE,
                },
                SurfaceEvent::KeyPressed(key) | SurfaceEvent::KeyReleased(key) => {
                    let Some(name) = egui::Key::from_name(&key.name) else {
                        continue;
                    };
                    egui::Event::Key {
                        key: name,
                        physical_key: None,
                        pressed: matches!(event, SurfaceEvent::KeyPressed(_)),
                        repeat: false,
                        modifiers: held(&key.modifiers),
                    }
                }
                SurfaceEvent::Text(typed) => egui::Event::Text(typed.text.clone()),
                SurfaceEvent::FocusLost(_) => egui::Event::WindowFocused(false),
            };
            self.events.push(event);
        }
    }

    /// Runs `ui` for this frame and draws what it made on the surface.
    ///
    /// # Errors
    /// What the GPU API refused.
    pub fn frame(&mut self, mut ui: impl FnMut(&egui::Context)) -> Result<(), Error> {
        let area = whole_screen();
        let scale = self.scale;
        #[allow(clippy::cast_precision_loss)]
        let screen = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(area.width as f32 / scale, area.height as f32 / scale),
        );
        let viewport = egui::ViewportInfo {
            native_pixels_per_point: Some(scale),
            ..Default::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(screen),
            time: Some(self.started.elapsed().as_secs_f64()),
            focused: true,
            events: std::mem::take(&mut self.events),
            viewports: std::iter::once((egui::ViewportId::ROOT, viewport)).collect(),
            ..Default::default()
        };
        let output = self.context.run_ui(input, |ui_root| ui(ui_root.ctx()));
        let primitives = self
            .context
            .tessellate(output.shapes, output.pixels_per_point);
        self.textures_delta(&output.textures_delta)?;
        // The mouse reaches the surface where egui has something.
        let input_rects: Vec<Rect> = self.context.memory(|memory| {
            memory
                .layer_ids()
                // What a plugin paints on the background, such as marks in
                // the world, lets the mouse through to the game.
                .filter(|layer| layer.order != egui::Order::Background)
                .filter_map(|layer| memory.area_rect(layer.id))
                .map(|rect| rect_of(rect, scale))
                .collect()
        });
        let keyboard = self.context.egui_wants_keyboard_input();
        gpu::set_surface(self.surface, area, 1.0, &input_rects, keyboard)?;
        self.area = area;
        self.draw(&primitives)?;
        for id in &output.textures_delta.free {
            if let Some(texture) = self.textures.remove(id) {
                gpu::destroy(texture.group);
                gpu::destroy(texture.image);
            }
        }
        Ok(())
    }

    fn textures_delta(&mut self, delta: &egui::TexturesDelta) -> Result<(), Error> {
        for (id, image) in delta
            .set
            .iter()
            .flat_map(|(id, images)| images.iter().map(move |image| (id, image)))
        {
            let egui::ImageData::Color(color) = &image.image;
            let [width, height] = color.size;
            let bytes: Vec<u8> = color
                .pixels
                .iter()
                .flat_map(egui::Color32::to_array)
                .collect();
            let to = |value: usize| u32::try_from(value).unwrap_or(u32::MAX);
            let replace = image.pos.is_none()
                || self
                    .textures
                    .get(id)
                    .is_none_or(|texture| texture.size != color.size);
            if replace {
                if let Some(old) = self.textures.remove(id) {
                    gpu::destroy(old.group);
                    gpu::destroy(old.image);
                }
                let texture =
                    gpu::create_texture(to(width), to(height), TextureFormat::Rgba8Unorm)?;
                let group = gpu::create_bind_group(&[
                    Binding {
                        index: 0,
                        bound: self.uniform,
                    },
                    Binding {
                        index: 1,
                        bound: texture,
                    },
                    Binding {
                        index: 2,
                        bound: self.sampler,
                    },
                ])?;
                self.textures.insert(
                    *id,
                    Texture {
                        image: texture,
                        group,
                        size: color.size,
                    },
                );
            }
            let [x, y] = image.pos.unwrap_or([0, 0]);
            if let Some(texture) = self.textures.get(id) {
                gpu::write_texture(
                    texture.image,
                    Rect {
                        x: i32::try_from(x).unwrap_or(0),
                        y: i32::try_from(y).unwrap_or(0),
                        width: to(width),
                        height: to(height),
                    },
                    &bytes,
                )?;
            }
        }
        Ok(())
    }

    fn draw(&mut self, primitives: &[egui::ClippedPrimitive]) -> Result<(), Error> {
        let scale = self.scale;
        let mut vertices: Vec<u8> = Vec::new();
        let mut indices: Vec<u8> = Vec::new();
        let mut commands = vec![Command::Pipeline(self.pipeline)];
        let mut first_index = 0u32;
        let mut base_vertex = 0i32;
        let mut draws = Vec::new();
        for primitive in primitives {
            let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive else {
                continue;
            };
            let Some(texture) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            if mesh.indices.is_empty() {
                continue;
            }
            for vertex in &mesh.vertices {
                for value in [
                    vertex.pos.x * scale,
                    vertex.pos.y * scale,
                    vertex.uv.x,
                    vertex.uv.y,
                ] {
                    vertices.extend_from_slice(&value.to_le_bytes());
                }
                vertices.extend_from_slice(&vertex.color.to_array());
            }
            indices.extend(mesh.indices.iter().flat_map(|index| index.to_le_bytes()));
            let count = u32::try_from(mesh.indices.len()).unwrap_or(0);
            draws.push((
                texture.group,
                rect_of(primitive.clip_rect, scale),
                count,
                first_index,
                base_vertex,
            ));
            first_index += count;
            base_vertex += i32::try_from(mesh.vertices.len()).unwrap_or(0);
        }
        #[allow(clippy::cast_precision_loss)]
        let size: Vec<u8> = [self.area.width as f32, self.area.height as f32, 0.0, 0.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        gpu::write_buffer(self.uniform, 0, &size)?;
        if !draws.is_empty() {
            let to = |bytes: &[u8]| u32::try_from(bytes.len()).unwrap_or(u32::MAX);
            let vertex_buffer = self.vertices.with_room(to(&vertices))?;
            let index_buffer = self.indices.with_room(to(&indices))?;
            gpu::write_buffer(vertex_buffer, 0, &vertices)?;
            gpu::write_buffer(index_buffer, 0, &indices)?;
            commands.push(Command::VertexBuffer(VertexBuffer {
                buffer: vertex_buffer,
                offset: 0,
            }));
            commands.push(Command::IndexBuffer(IndexBuffer {
                buffer: index_buffer,
                offset: 0,
                wide: true,
            }));
            for (group, clip, count, first, base) in draws {
                commands.push(Command::BindGroup(group));
                commands.push(Command::Scissor(clip));
                commands.push(Command::DrawIndexed(DrawIndexed {
                    indices: count,
                    instances: 1,
                    first_index: first,
                    base_vertex: base,
                    first_instance: 0,
                }));
            }
        }
        gpu::render(
            self.surface,
            Some(gpu::Color {
                r: 0.0,
                g: 0.0,
                b: 0.0,
                a: 0.0,
            }),
            &commands,
        )
    }
}
