//! Plugins' surfaces and the GPU objects they draw them with
//! (`wit/gpu.wit`). The main instance creates objects and renders surfaces
//! on the game's main thread; this module checks each request and queues
//! it as an operation, which the renderer runs on the thread that presents
//! the game's frames, in order, before it composites the surfaces over the
//! game. The checks are what keeps the game safe and plugins apart: an
//! object belongs to its plugin, a shader is valid WGSL, sizes and counts
//! stay within limits. What a plugin draws is its own.
#![forbid(unsafe_code)]
use crate::error::{Error, Kind, Result};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};

/// GPU memory one plugin's objects may take.
pub const MOST_MEMORY: u64 = 256 * 1024 * 1024;
/// Objects one plugin may hold at once.
pub const MOST_OBJECTS: usize = 4096;
/// Draws one plugin may make between two frames the game presents.
pub const MOST_DRAWS: usize = 10_000;
/// The largest buffer.
pub const MOST_BUFFER: u32 = 64 * 1024 * 1024;
/// The largest side of a texture or a surface.
pub const MOST_SIDE: u32 = 8192;
/// Bindings and vertex locations a shader may use: 0 to 7.
pub const MOST_SLOTS: u32 = 8;
/// The largest vertex stride.
pub const MOST_STRIDE: u32 = 2048;

/// An object's id between this module and the renderer, unique in the
/// process.
pub type Id = u64;

/// Compiles an entry point of HLSL for a vertex stage or a pixel stage.
pub type Compile<'a> = &'a dyn Fn(&str, &str, bool) -> std::result::Result<Vec<u8>, String>;

/// A rectangle in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    /// Whether the point lies inside.
    #[must_use]
    pub fn contains(&self, x: f32, y: f32) -> bool {
        #[allow(clippy::cast_precision_loss)]
        let (left, top) = (self.x as f32, self.y as f32);
        #[allow(clippy::cast_precision_loss)]
        let (right, bottom) = (left + self.width as f32, top + self.height as f32);
        x >= left && x < right && y >= top && y < bottom
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub vertex: bool,
    pub index: bool,
    pub uniform: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Rgba8,
    Rgba8Srgb,
    R8,
}

impl Format {
    #[must_use]
    pub fn bytes(self) -> u32 {
        match self {
            Self::Rgba8 | Self::Rgba8Srgb => 4,
            Self::R8 => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Filter {
    Nearest,
    Linear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    Clamp,
    Repeat,
    Mirror,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VertexFormat {
    Float32,
    Float32x2,
    Float32x3,
    Float32x4,
    Unorm8x4,
    Uint32,
    Sint32,
}

impl VertexFormat {
    #[must_use]
    pub fn bytes(self) -> u32 {
        match self {
            Self::Float32 | Self::Unorm8x4 | Self::Uint32 | Self::Sint32 => 4,
            Self::Float32x2 => 8,
            Self::Float32x3 => 12,
            Self::Float32x4 => 16,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Attribute {
    pub location: u32,
    pub format: VertexFormat,
    pub offset: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topology {
    TriangleList,
    TriangleStrip,
    LineList,
    LineStrip,
    PointList,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Replace,
    Alpha,
    Premultiplied,
    Additive,
}

/// What a binding of a bind group holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resource {
    Uniform(Id),
    Texture(Id),
    Sampler(Id),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Pipeline(Id),
    BindGroup(Id),
    VertexBuffer {
        buffer: Id,
        offset: u32,
    },
    IndexBuffer {
        buffer: Id,
        offset: u32,
        wide: bool,
    },
    Scissor(Rect),
    Draw {
        vertices: u32,
        instances: u32,
        first_vertex: u32,
        first_instance: u32,
    },
    DrawIndexed {
        indices: u32,
        instances: u32,
        first_index: u32,
        base_vertex: i32,
        first_instance: u32,
    },
}

/// What the renderer does, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Buffer {
        id: Id,
        size: u32,
        usage: Usage,
    },
    WriteBuffer {
        id: Id,
        offset: u32,
        bytes: Vec<u8>,
    },
    Texture {
        id: Id,
        width: u32,
        height: u32,
        format: Format,
    },
    WriteTexture {
        id: Id,
        area: Rect,
        format: Format,
        bytes: Vec<u8>,
    },
    Sampler {
        id: Id,
        filter: Filter,
        address: Address,
    },
    Pipeline {
        id: Id,
        /// The compiled vertex and pixel shaders.
        vertex: Arc<Vec<u8>>,
        fragment: Arc<Vec<u8>>,
        stride: u32,
        attributes: Vec<Attribute>,
        topology: Topology,
        blend: Blend,
    },
    BindGroup {
        id: Id,
        bindings: Vec<(u32, Resource)>,
    },
    /// A surface's image, made again at a new size; cleared to transparent.
    Surface {
        id: Id,
        width: u32,
        height: u32,
    },
    Render {
        surface: Id,
        clear: Option<[f32; 4]>,
        commands: Vec<Command>,
    },
    Destroy(Id),
}

/// A surface as the renderer composites it.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    pub id: Id,
    pub plugin: String,
    pub area: Rect,
    pub opacity: f32,
    /// Where it takes the mouse, in its own pixels.
    pub input: Vec<Rect>,
    pub keyboard: bool,
}

/// One shader entry point: its stage and its names in WGSL and HLSL.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    vertex: bool,
    wgsl: String,
    hlsl: String,
    /// The vertex inputs' locations.
    locations: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq)]
enum Class {
    Surface,
    Buffer(Usage),
    Texture(Format, u32, u32),
    Sampler,
    Shader(Arc<String>, Vec<Entry>),
    Pipeline,
    BindGroup,
}

#[derive(Debug, Clone)]
struct Object {
    id: Id,
    kind: Class,
    bytes: u64,
}

#[derive(Default)]
struct Owned {
    objects: HashMap<u32, Object>,
    next: u32,
    memory: u64,
    draws: usize,
}

#[derive(Default)]
struct Shared {
    next: Id,
    plugins: HashMap<String, Owned>,
    ops: Vec<Op>,
    /// Surfaces from the bottom up.
    placed: Vec<Placed>,
    focused: Option<Id>,
    screen: (u32, u32),
    camera: Option<[f32; 16]>,
    pointer: input::Pointer,
    /// Input on each plugin's surfaces that it has yet to take.
    input: HashMap<String, Vec<input::Event>>,
}

/// Every plugin's surfaces and GPU objects, and what the renderer has yet
/// to do.
#[derive(Default)]
pub struct Gpu {
    shared: Mutex<Shared>,
    /// The player's theme and the scale of Angelica's windows, as the menu
    /// drew them last.
    look: Mutex<(bool, f32)>,
}

/// What the renderer takes each frame: the operations queued since, and
/// the surfaces to composite from the bottom up.
#[derive(Debug, Default)]
pub struct Frame {
    pub ops: Vec<Op>,
    pub placed: Vec<Placed>,
}

fn invalid(message: impl Into<String>) -> Error {
    Error::new(Kind::InvalidArgument, message)
}

impl Owned {
    fn get(&self, object: u32) -> Result<&Object> {
        self.objects.get(&object).ok_or_else(|| {
            Error::new(
                Kind::NotFound,
                format!("object {object} is not the plugin's"),
            )
        })
    }

    fn of(&self, object: u32, what: &str, test: impl Fn(&Class) -> bool) -> Result<&Object> {
        let found = self.get(object)?;
        if test(&found.kind) {
            Ok(found)
        } else {
            Err(invalid(format!("object {object} is not a {what}")))
        }
    }
}

impl Shared {
    /// The plugin of the object `id`, and its id there.
    fn local_of(&self, id: Id) -> Option<(String, u32)> {
        self.plugins.iter().find_map(|(plugin, owned)| {
            owned
                .objects
                .iter()
                .find(|(_, object)| object.id == id)
                .map(|(local, _)| (plugin.clone(), *local))
        })
    }

    fn owned(&mut self, plugin: &str) -> &mut Owned {
        self.plugins.entry(plugin.to_owned()).or_default()
    }

    /// Takes `bytes` of the plugin's memory and gives a new object's ids.
    fn add(&mut self, plugin: &str, kind: Class, bytes: u64) -> Result<(u32, Id)> {
        self.next += 1;
        let id = self.next;
        let owned = self.owned(plugin);
        if owned.objects.len() >= MOST_OBJECTS {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin holds at most {MOST_OBJECTS} GPU objects"),
            ));
        }
        if owned.memory + bytes > MOST_MEMORY {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!(
                    "a plugin's GPU objects take at most {} MiB",
                    MOST_MEMORY >> 20
                ),
            ));
        }
        owned.next += 1;
        let local = owned.next;
        owned.memory += bytes;
        owned.objects.insert(local, Object { id, kind, bytes });
        Ok((local, id))
    }
}

fn side(value: u32, what: &str) -> Result<u32> {
    if value == 0 || value > MOST_SIDE {
        Err(invalid(format!("{what} is 1 to {MOST_SIDE} pixels")))
    } else {
        Ok(value)
    }
}

impl Gpu {
    fn shared(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The player's theme, dark or light, and how many times its normal
    /// size Angelica's menu is drawn.
    pub fn set_look(&self, dark: bool, scale: f32) {
        *self.look.lock().unwrap_or_else(PoisonError::into_inner) = (dark, scale);
    }

    #[must_use]
    pub fn look(&self) -> (bool, f32) {
        *self.look.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The game's window and camera this frame, as the renderer saw them.
    pub fn set_view(&self, screen: (u32, u32), camera: Option<[f32; 16]>) {
        let mut shared = self.shared();
        shared.screen = screen;
        shared.camera = camera;
    }

    #[must_use]
    pub fn screen(&self) -> (u32, u32) {
        self.shared().screen
    }

    #[must_use]
    pub fn camera(&self) -> Option<[f32; 16]> {
        self.shared().camera
    }

    /// What the renderer does this frame: the operations queued since the
    /// last, and the surfaces from the bottom up. Each plugin's draws count
    /// again from here.
    pub fn take(&self) -> Frame {
        let mut shared = self.shared();
        for owned in shared.plugins.values_mut() {
            owned.draws = 0;
        }
        Frame {
            ops: std::mem::take(&mut shared.ops),
            placed: shared.placed.clone(),
        }
    }

    /// The surfaces from the bottom up.
    #[must_use]
    pub fn placed(&self) -> Vec<Placed> {
        self.shared().placed.clone()
    }

    /// Puts the surface `id` on top and gives it focus; none takes focus
    /// from every surface. Gives the surface that lost focus.
    pub fn focus(&self, id: Option<Id>) -> Option<Placed> {
        let mut shared = self.shared();
        if shared.focused == id {
            return None;
        }
        let lost = shared.focused.and_then(|old| {
            shared
                .placed
                .iter()
                .find(|placed| placed.id == old)
                .cloned()
        });
        shared.focused = id;
        if let Some(id) = id
            && let Some(at) = shared.placed.iter().position(|placed| placed.id == id)
        {
            let placed = shared.placed.remove(at);
            shared.placed.push(placed);
        }
        lost
    }

    /// The plugin's id of the object `id`, and the plugin.
    #[must_use]
    pub fn local(&self, id: Id) -> Option<(String, u32)> {
        self.shared().local_of(id)
    }

    /// The surface the player clicked last of `plugin`'s, when it is.
    #[must_use]
    pub fn focused(&self, plugin: &str) -> Option<u32> {
        let id = self.shared().focused?;
        self.local(id)
            .filter(|(owner, _)| owner == plugin)
            .map(|(_, local)| local)
    }

    /// Ends every object of `plugin`.
    pub fn remove_plugin(&self, plugin: &str) {
        let mut shared = self.shared();
        let Some(owned) = shared.plugins.remove(plugin) else {
            return;
        };
        for object in owned.objects.values() {
            shared.ops.push(Op::Destroy(object.id));
        }
        shared.placed.retain(|placed| placed.plugin != plugin);
        shared.input.remove(plugin);
        if shared
            .focused
            .is_some_and(|id| owned.objects.values().any(|object| object.id == id))
        {
            shared.focused = None;
        }
    }

    /// # Errors
    /// A size of 0 or past 8192, and the plugin's limits.
    pub fn create_surface(&self, plugin: &str, area: Rect) -> Result<u32> {
        let width = side(area.width, "a surface's width")?;
        let height = side(area.height, "a surface's height")?;
        let mut shared = self.shared();
        let (local, id) = shared.add(
            plugin,
            Class::Surface,
            u64::from(width) * u64::from(height) * 4,
        )?;
        shared.ops.push(Op::Surface { id, width, height });
        shared.placed.push(Placed {
            id,
            plugin: plugin.to_owned(),
            area,
            opacity: 1.0,
            input: Vec::new(),
            keyboard: false,
        });
        Ok(local)
    }

    /// # Errors
    /// As [`Gpu::create_surface`], more than 64 input rectangles, and an object that is not one of the plugin's surfaces.
    pub fn set_surface(
        &self,
        plugin: &str,
        surface: u32,
        area: Rect,
        opacity: f32,
        input: Vec<Rect>,
        keyboard: bool,
    ) -> Result<()> {
        let width = side(area.width, "a surface's width")?;
        let height = side(area.height, "a surface's height")?;
        if input.len() > 64 {
            return Err(invalid(
                "a surface takes the mouse in at most 64 rectangles",
            ));
        }
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let object = owned.of(surface, "surface", |kind| *kind == Class::Surface)?;
        let id = object.id;
        let bytes = u64::from(width) * u64::from(height) * 4;
        let grown = bytes.saturating_sub(object.bytes);
        if owned.memory + grown > MOST_MEMORY {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!(
                    "a plugin's GPU objects take at most {} MiB",
                    MOST_MEMORY >> 20
                ),
            ));
        }
        let old = object.bytes;
        owned.memory = owned.memory - old + bytes;
        if let Some(object) = owned.objects.get_mut(&surface) {
            object.bytes = bytes;
        }
        let Some(placed) = shared.placed.iter_mut().find(|placed| placed.id == id) else {
            return Err(Error::new(Kind::NotFound, "the surface is gone"));
        };
        let resized = placed.area.width != width || placed.area.height != height;
        placed.area = area;
        placed.opacity = opacity.clamp(0.0, 1.0);
        placed.input = input;
        placed.keyboard = keyboard;
        if resized {
            shared.ops.push(Op::Surface { id, width, height });
        }
        Ok(())
    }

    pub fn destroy(&self, plugin: &str, object: u32) {
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let Some(found) = owned.objects.remove(&object) else {
            return;
        };
        owned.memory -= found.bytes;
        shared.placed.retain(|placed| placed.id != found.id);
        if shared.focused == Some(found.id) {
            shared.focused = None;
        }
        shared.ops.push(Op::Destroy(found.id));
    }

    /// # Errors
    /// A size or a usage out of range, and the plugin's limits.
    pub fn create_buffer(&self, plugin: &str, size: u32, usage: Usage) -> Result<u32> {
        if size == 0 || size > MOST_BUFFER {
            return Err(invalid(format!("a buffer has 1 to {MOST_BUFFER} bytes")));
        }
        if !(usage.vertex || usage.index || usage.uniform) {
            return Err(invalid("a buffer has a usage"));
        }
        if usage.uniform && (usage.vertex || usage.index) {
            return Err(invalid("a uniform buffer has no other usage"));
        }
        if usage.uniform && (!size.is_multiple_of(16) || size > 65536) {
            return Err(invalid(
                "a uniform buffer's size is a multiple of 16, at most 65536",
            ));
        }
        let mut shared = self.shared();
        let (local, id) = shared.add(plugin, Class::Buffer(usage), u64::from(size))?;
        shared.ops.push(Op::Buffer { id, size, usage });
        Ok(local)
    }

    /// # Errors
    /// A write past the buffer's end, and an object that is not one of the plugin's buffers.
    pub fn write_buffer(
        &self,
        plugin: &str,
        buffer: u32,
        offset: u32,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let object = owned.of(buffer, "buffer", |kind| matches!(kind, Class::Buffer(_)))?;
        let end = u64::from(offset) + bytes.len() as u64;
        if end > object.bytes {
            return Err(invalid(format!(
                "the write ends at {end}, past the buffer's {} bytes",
                object.bytes
            )));
        }
        let id = object.id;
        shared.ops.push(Op::WriteBuffer { id, offset, bytes });
        Ok(())
    }

    /// # Errors
    /// A size of 0 or past 8192, and the plugin's limits.
    pub fn create_texture(
        &self,
        plugin: &str,
        width: u32,
        height: u32,
        format: Format,
    ) -> Result<u32> {
        side(width, "a texture's width")?;
        side(height, "a texture's height")?;
        let mut shared = self.shared();
        let (local, id) = shared.add(
            plugin,
            Class::Texture(format, width, height),
            u64::from(width) * u64::from(height) * u64::from(format.bytes()),
        )?;
        shared.ops.push(Op::Texture {
            id,
            width,
            height,
            format,
        });
        Ok(local)
    }

    /// # Errors
    /// An area outside the texture, bytes that do not fill it, and an object that is not one of the plugin's textures.
    pub fn write_texture(
        &self,
        plugin: &str,
        texture: u32,
        area: Rect,
        bytes: Vec<u8>,
    ) -> Result<()> {
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let object = owned.of(texture, "texture", |kind| {
            matches!(kind, Class::Texture(..))
        })?;
        let Class::Texture(format, width, height) = object.kind else {
            return Err(invalid("not a texture"));
        };
        let x = u32::try_from(area.x).map_err(|_| invalid("the area starts inside the texture"))?;
        let y = u32::try_from(area.y).map_err(|_| invalid("the area starts inside the texture"))?;
        if area.width == 0
            || area.height == 0
            || u64::from(x) + u64::from(area.width) > u64::from(width)
            || u64::from(y) + u64::from(area.height) > u64::from(height)
        {
            return Err(invalid(format!(
                "the area lies outside the texture of {width} by {height}"
            )));
        }
        let expected = u64::from(area.width) * u64::from(area.height) * u64::from(format.bytes());
        if bytes.len() as u64 != expected {
            return Err(invalid(format!(
                "the area takes {expected} bytes, not {}",
                bytes.len()
            )));
        }
        let id = object.id;
        shared.ops.push(Op::WriteTexture {
            id,
            area,
            format,
            bytes,
        });
        Ok(())
    }

    /// # Errors
    /// The plugin's limits.
    pub fn create_sampler(&self, plugin: &str, filter: Filter, address: Address) -> Result<u32> {
        let mut shared = self.shared();
        let (local, id) = shared.add(plugin, Class::Sampler, 0)?;
        shared.ops.push(Op::Sampler {
            id,
            filter,
            address,
        });
        Ok(local)
    }

    /// # Errors
    /// WGSL that does not translate, with the reason, and the plugin's limits.
    pub fn create_shader(&self, plugin: &str, wgsl: &str) -> Result<u32> {
        let (hlsl, entries) = translate(wgsl)?;
        let mut shared = self.shared();
        let (local, _) = shared.add(plugin, Class::Shader(Arc::new(hlsl), entries), 0)?;
        Ok(local)
    }

    /// A pipeline of a shader's entry points `vertex` and `fragment`,
    /// which `compile` turns from HLSL into the GPU's code.
    ///
    /// # Errors
    /// Entry points the shader lacks, vertex inputs no attribute gives,
    /// attributes out of range, a shader that does not compile, and the
    /// plugin's limits.
    #[allow(clippy::too_many_arguments)]
    pub fn create_pipeline(
        &self,
        plugin: &str,
        shader: u32,
        vertex: &str,
        fragment: &str,
        stride: u32,
        attributes: Vec<Attribute>,
        topology: Topology,
        blend: Blend,
        compile: Compile<'_>,
    ) -> Result<u32> {
        if stride > MOST_STRIDE {
            return Err(invalid(format!("a vertex stride is at most {MOST_STRIDE}")));
        }
        let mut seen = Vec::new();
        for attribute in &attributes {
            if attribute.location >= MOST_SLOTS || seen.contains(&attribute.location) {
                return Err(invalid(format!(
                    "vertex locations are 0 to {} and each once",
                    MOST_SLOTS - 1
                )));
            }
            if attribute.offset + attribute.format.bytes() > stride {
                return Err(invalid(format!(
                    "the attribute at location {} ends past the stride",
                    attribute.location
                )));
            }
            seen.push(attribute.location);
        }
        let (hlsl, entries) = {
            let mut shared = self.shared();
            let owned = shared.owned(plugin);
            let object = owned.of(shader, "shader", |kind| matches!(kind, Class::Shader(..)))?;
            let Class::Shader(hlsl, entries) = &object.kind else {
                return Err(invalid("not a shader"));
            };
            (hlsl.clone(), entries.clone())
        };
        let find = |name: &str, vertex: bool| {
            entries
                .iter()
                .find(|entry| entry.wgsl == name && entry.vertex == vertex)
                .cloned()
                .ok_or_else(|| {
                    invalid(format!(
                        "the shader has no {} entry point {name}",
                        if vertex { "vertex" } else { "fragment" }
                    ))
                })
        };
        let vertex_entry = find(vertex, true)?;
        let fragment_entry = find(fragment, false)?;
        if let Some(missing) = vertex_entry
            .locations
            .iter()
            .find(|location| !seen.contains(location))
        {
            return Err(invalid(format!(
                "the vertex entry point reads location {missing}, which no attribute gives"
            )));
        }
        let failed = |error: String| invalid(format!("the shader does not compile: {error}"));
        let vertex_code = compile(&hlsl, &vertex_entry.hlsl, true).map_err(failed)?;
        let fragment_code = compile(&hlsl, &fragment_entry.hlsl, false).map_err(failed)?;
        let mut shared = self.shared();
        let (local, id) = shared.add(plugin, Class::Pipeline, 0)?;
        shared.ops.push(Op::Pipeline {
            id,
            vertex: Arc::new(vertex_code),
            fragment: Arc::new(fragment_code),
            stride,
            attributes,
            topology,
            blend,
        });
        Ok(local)
    }

    /// # Errors
    /// Bindings out of range or twice, objects of other kinds or not the plugin's, and the plugin's limits.
    pub fn create_bind_group(&self, plugin: &str, bindings: &[(u32, u32)]) -> Result<u32> {
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let mut resolved = Vec::new();
        for (index, object) in bindings {
            if *index >= MOST_SLOTS || resolved.iter().any(|(at, _)| at == index) {
                return Err(invalid(format!(
                    "bindings are 0 to {} and each once",
                    MOST_SLOTS - 1
                )));
            }
            let found = owned.get(*object)?;
            let resource = match found.kind {
                Class::Buffer(usage) if usage.uniform => Resource::Uniform(found.id),
                Class::Texture(..) => Resource::Texture(found.id),
                Class::Sampler => Resource::Sampler(found.id),
                _ => {
                    return Err(invalid(format!(
                        "object {object} is not a uniform buffer, a texture, or a sampler"
                    )));
                }
            };
            resolved.push((*index, resource));
        }
        let (local, id) = shared.add(plugin, Class::BindGroup, 0)?;
        shared.ops.push(Op::BindGroup {
            id,
            bindings: resolved,
        });
        Ok(local)
    }

    /// Queues the plugin's commands for `surface`. A clear drops the
    /// commands queued for the surface that the renderer has not run yet.
    ///
    /// # Errors
    /// Objects that are not the plugin's or of the right kind, a draw
    /// before a pipeline, and more draws than a frame allows.
    pub fn render(
        &self,
        plugin: &str,
        surface: u32,
        clear: Option<[f32; 4]>,
        commands: Vec<Local>,
    ) -> Result<()> {
        let mut shared = self.shared();
        let owned = shared.owned(plugin);
        let target = owned
            .of(surface, "surface", |kind| *kind == Class::Surface)?
            .id;
        let mut resolved = Vec::with_capacity(commands.len());
        let mut pipeline = false;
        let mut draws = 0;
        for command in commands {
            let of = |object: u32, what: &str, test: &dyn Fn(&Class) -> bool| {
                owned.of(object, what, test).map(|found| found.id)
            };
            resolved.push(match command {
                Local::Pipeline(object) => {
                    pipeline = true;
                    Command::Pipeline(of(object, "pipeline", &|kind| *kind == Class::Pipeline)?)
                }
                Local::BindGroup(object) => {
                    Command::BindGroup(of(object, "bind group", &|kind| *kind == Class::BindGroup)?)
                }
                Local::VertexBuffer { buffer, offset } => Command::VertexBuffer {
                    buffer: of(
                        buffer,
                        "vertex buffer",
                        &|kind| matches!(kind, Class::Buffer(usage) if usage.vertex),
                    )?,
                    offset,
                },
                Local::IndexBuffer {
                    buffer,
                    offset,
                    wide,
                } => Command::IndexBuffer {
                    buffer: of(
                        buffer,
                        "index buffer",
                        &|kind| matches!(kind, Class::Buffer(usage) if usage.index),
                    )?,
                    offset,
                    wide,
                },
                Local::Scissor(rect) => Command::Scissor(rect),
                Local::Draw(draw) => {
                    if !pipeline {
                        return Err(invalid("a draw needs a pipeline first"));
                    }
                    draws += 1;
                    draw
                }
            });
        }
        if owned.draws + draws > MOST_DRAWS {
            return Err(Error::new(
                Kind::LimitExceeded,
                format!("a plugin makes at most {MOST_DRAWS} draws a frame"),
            ));
        }
        owned.draws += draws;
        if clear.is_some() {
            shared
                .ops
                .retain(|op| !matches!(op, Op::Render { surface, .. } if *surface == target));
        }
        shared.ops.push(Op::Render {
            surface: target,
            clear,
            commands: resolved,
        });
        Ok(())
    }
}

/// A render command with the plugin's own ids; a draw is resolved already.
#[derive(Debug, Clone, PartialEq)]
pub enum Local {
    Pipeline(u32),
    BindGroup(u32),
    VertexBuffer {
        buffer: u32,
        offset: u32,
    },
    IndexBuffer {
        buffer: u32,
        offset: u32,
        wide: bool,
    },
    Scissor(Rect),
    Draw(Command),
}

/// A WGSL module as HLSL for shader model 5.0, with its entry points.
///
/// # Errors
/// WGSL that does not parse or validate, resources outside group 0 or
/// bindings 0 to 7, vertex inputs past location 7, and stages other than
/// vertex and fragment.
fn translate(wgsl: &str) -> Result<(String, Vec<Entry>)> {
    use naga::back::hlsl;
    let module = naga::front::wgsl::parse_str(wgsl).map_err(|error| {
        invalid(format!(
            "the shader does not parse: {}",
            error.emit_to_string(wgsl)
        ))
    })?;
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|error| {
        invalid(format!(
            "the shader is not valid: {}",
            error.emit_to_string(wgsl)
        ))
    })?;
    let mut binding_map = hlsl::BindingMap::default();
    for (_, variable) in module.global_variables.iter() {
        if let Some(binding) = &variable.binding {
            if binding.group != 0 || binding.binding >= MOST_SLOTS {
                return Err(invalid(format!(
                    "the shader's resources are in group 0 at bindings 0 to {}",
                    MOST_SLOTS - 1
                )));
            }
            binding_map.insert(
                binding.clone(),
                hlsl::BindTarget {
                    space: 0,
                    register: binding.binding,
                    binding_array_size: None,
                },
            );
        }
    }
    let options = hlsl::Options {
        shader_model: hlsl::ShaderModel::V5_0,
        binding_map,
        fake_missing_bindings: false,
        ..Default::default()
    };
    let mut hlsl_text = String::new();
    let reflection = hlsl::Writer::new(&mut hlsl_text, &options)
        .write(&module, &info, None)
        .map_err(|error| invalid(format!("the shader cannot run on this GPU: {error}")))?;
    let mut entries = Vec::new();
    for (point, name) in module.entry_points.iter().zip(reflection.entry_point_names) {
        let vertex = match point.stage {
            naga::ShaderStage::Vertex => true,
            naga::ShaderStage::Fragment => false,
            naga::ShaderStage::Compute => continue,
        };
        let name = name.map_err(|error| invalid(format!("entry point {}: {error}", point.name)))?;
        let mut locations = Vec::new();
        if vertex {
            for argument in &point.function.arguments {
                collect_locations(
                    &module,
                    argument.binding.as_ref(),
                    argument.ty,
                    &mut locations,
                );
            }
            if let Some(far) = locations.iter().find(|location| **location >= MOST_SLOTS) {
                return Err(invalid(format!(
                    "a vertex input is at location {far}; locations are 0 to {}",
                    MOST_SLOTS - 1
                )));
            }
        }
        entries.push(Entry {
            vertex,
            wgsl: point.name.clone(),
            hlsl: name,
            locations,
        });
    }
    Ok((hlsl_text, entries))
}

/// The locations of a vertex entry point's argument, or of its members.
fn collect_locations(
    module: &naga::Module,
    binding: Option<&naga::Binding>,
    ty: naga::Handle<naga::Type>,
    locations: &mut Vec<u32>,
) {
    match binding {
        Some(naga::Binding::Location { location, .. }) => locations.push(*location),
        Some(naga::Binding::BuiltIn(_)) => {}
        None => {
            if let naga::TypeInner::Struct { members, .. } = &module.types[ty].inner {
                for member in members {
                    collect_locations(module, member.binding.as_ref(), member.ty, locations);
                }
            }
        }
    }
}

mod api;
pub mod input;
#[cfg(test)]
mod tests;

/// The handler of input that a plugin with surfaces exports.
pub mod bindings {
    wasmtime::component::bindgen!({
        path: "../../wit",
        world: "surface-input",
        with: {
            "angelica:plugin/gpu": crate::sandbox::bindings::angelica::plugin::gpu,
            "angelica:plugin/types": crate::sandbox::bindings::angelica::plugin::types,
        },
    });
    use super::input::{Button, Event, Modifiers};
    use exports::angelica::plugin::surface_handler as api;

    fn button(button: Button) -> api::Button {
        match button {
            Button::Left => api::Button::Left,
            Button::Right => api::Button::Right,
            Button::Middle => api::Button::Middle,
        }
    }

    fn modifiers(modifiers: Modifiers) -> api::Modifiers {
        api::Modifiers {
            shift: modifiers.shift,
            ctrl: modifiers.ctrl,
            alt: modifiers.alt,
        }
    }

    /// An event as the plugin receives it.
    #[must_use]
    pub fn event(event: Event) -> api::SurfaceEvent {
        match event {
            Event::Moved { surface, x, y } => {
                api::SurfaceEvent::PointerMoved(api::Pointer { surface, x, y })
            }
            Event::Left(surface) => api::SurfaceEvent::PointerLeft(surface),
            Event::Button {
                surface,
                x,
                y,
                button: which,
                pressed,
                modifiers: held,
            } => {
                let press = api::Press {
                    surface,
                    x,
                    y,
                    button: button(which),
                    modifiers: modifiers(held),
                };
                if pressed {
                    api::SurfaceEvent::Pressed(press)
                } else {
                    api::SurfaceEvent::Released(press)
                }
            }
            Event::Wheel { surface, x, y } => {
                api::SurfaceEvent::Wheel(api::Wheel { surface, x, y })
            }
            Event::Key {
                surface,
                name,
                pressed,
                modifiers: held,
            } => {
                let key = api::Key {
                    surface,
                    name,
                    modifiers: modifiers(held),
                };
                if pressed {
                    api::SurfaceEvent::KeyPressed(key)
                } else {
                    api::SurfaceEvent::KeyReleased(key)
                }
            }
            Event::Text { surface, text } => api::SurfaceEvent::Text(api::Typed { surface, text }),
            Event::FocusLost(surface) => api::SurfaceEvent::FocusLost(surface),
        }
    }
}
