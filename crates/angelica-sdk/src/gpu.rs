//! Surfaces over the game that the plugin draws on with the GPU, shaped
//! after WebGPU: buffers, textures, samplers, WGSL shaders, pipelines, and
//! bind groups, and a list of commands per surface each frame. The host
//! composites the plugin's surfaces over the game, the one the player
//! clicked last on top, and passes the player's input on them to
//! [`crate::Plugin::surface_input`].
//!
//! ```ignore
//! use angelica_sdk::gpu::{self, Rect};
//!
//! let surface = gpu::create_surface(Rect { x: 100, y: 100, width: 300, height: 200 })?;
//! gpu::set_surface(surface, Rect { x: 100, y: 100, width: 300, height: 200 }, 1.0,
//!     &[Rect { x: 0, y: 0, width: 300, height: 200 }], false)?;
//! // Every frame:
//! gpu::render(surface, Some(gpu::Color { r: 0.1, g: 0.1, b: 0.1, a: 0.8 }), &commands)?;
//! ```
//!
//! The [`angelica-kit`] crate draws Angelica's interface on a surface.
//!
//! [`angelica-kit`]: https://github.com/AngelicaProject/angelica/tree/main/crates/angelica-kit

use crate::Error;
pub use crate::api::gpu::{
    AddressMode, Binding, Blend, BufferUsage, Color, Command, Draw, DrawIndexed, Filter,
    IndexBuffer, Object, PipelineDesc, Rect, Size, TextureFormat, Topology, VertexAttribute,
    VertexBuffer, VertexFormat,
};
pub use crate::bindings::exports::angelica::plugin::surface_handler::{
    Button, Key, Modifiers, Pointer, Press, SurfaceEvent, Typed, Wheel,
};

/// The size of the game's window in pixels.
#[must_use]
pub fn screen() -> Size {
    crate::api::gpu::screen()
}

/// The game camera's view and projection this frame, in rows.
#[must_use]
pub fn camera() -> Option<[f32; 16]> {
    crate::api::gpu::camera().and_then(|matrix| matrix.try_into().ok())
}

/// Where a point of the world is on the screen, in pixels, with the
/// camera of this frame; none behind the camera or without one.
#[must_use]
pub fn project(point: [f32; 3]) -> Option<[f32; 2]> {
    let m = camera()?;
    let size = screen();
    let [x, y, z] = point;
    // Row vector times the matrix.
    let clip = [
        x * m[0] + y * m[4] + z * m[8] + m[12],
        x * m[1] + y * m[5] + z * m[9] + m[13],
        x * m[2] + y * m[6] + z * m[10] + m[14],
        x * m[3] + y * m[7] + z * m[11] + m[15],
    ];
    if clip[3] <= f32::EPSILON {
        return None;
    }
    #[allow(clippy::cast_precision_loss)]
    let (width, height) = (size.width as f32, size.height as f32);
    Some([
        (clip[0] / clip[3] * 0.5 + 0.5) * width,
        (0.5 - clip[1] / clip[3] * 0.5) * height,
    ])
}

/// A surface over `area`, at the top.
///
/// # Errors
/// An area of no size or larger than 8192, and the plugin's limits.
pub fn create_surface(area: Rect) -> Result<Object, Error> {
    crate::api::gpu::create_surface(area)
}

/// Moves a surface, sets its opacity, where it takes the mouse in its own
/// pixels, and whether it takes the keyboard while it has focus.
///
/// # Errors
/// As [`create_surface`], and an object that is not one of its surfaces.
pub fn set_surface(
    surface: Object,
    area: Rect,
    opacity: f32,
    input: &[Rect],
    keyboard: bool,
) -> Result<(), Error> {
    crate::api::gpu::set_surface(surface, area, opacity, input, keyboard)
}

/// The plugin's surface the player clicked last, when it is on top.
#[must_use]
pub fn focused() -> Option<Object> {
    crate::api::gpu::focused()
}

/// Ends an object.
pub fn destroy(object: Object) {
    crate::api::gpu::destroy(object);
}

/// A buffer of `size` bytes.
///
/// # Errors
/// A size out of range, a usage that does not fit, and the plugin's limits.
pub fn create_buffer(size: u32, usage: BufferUsage) -> Result<Object, Error> {
    crate::api::gpu::create_buffer(size, usage)
}

/// Writes `bytes` into a buffer at `offset`.
///
/// # Errors
/// A write past the end, and an object that is not one of its buffers.
pub fn write_buffer(buffer: Object, offset: u32, bytes: &[u8]) -> Result<(), Error> {
    crate::api::gpu::write_buffer(buffer, offset, bytes)
}

/// A texture of `width` by `height` texels.
///
/// # Errors
/// A size out of range, and the plugin's limits.
pub fn create_texture(width: u32, height: u32, format: TextureFormat) -> Result<Object, Error> {
    crate::api::gpu::create_texture(width, height, format)
}

/// Writes texels into a texture's `area`, rows without gaps.
///
/// # Errors
/// An area outside the texture, and bytes that do not fill it exactly.
pub fn write_texture(texture: Object, area: Rect, bytes: &[u8]) -> Result<(), Error> {
    crate::api::gpu::write_texture(texture, area, bytes)
}

/// A sampler.
///
/// # Errors
/// The plugin's limits.
pub fn create_sampler(filter: Filter, address: AddressMode) -> Result<Object, Error> {
    crate::api::gpu::create_sampler(filter, address)
}

/// A shader module from WGSL.
///
/// # Errors
/// Source that does not parse or validate, with the reason.
pub fn create_shader(wgsl: &str) -> Result<Object, Error> {
    crate::api::gpu::create_shader(wgsl)
}

/// A render pipeline.
///
/// # Errors
/// Entry points the shader lacks, vertex inputs no attribute gives, and a
/// shader the GPU cannot compile, with the compiler's message.
pub fn create_pipeline(desc: &PipelineDesc) -> Result<Object, Error> {
    crate::api::gpu::create_pipeline(desc)
}

/// A bind group of uniform buffers, textures, and samplers by binding.
///
/// # Errors
/// Bindings out of range or twice, and objects of other kinds.
pub fn create_bind_group(bindings: &[Binding]) -> Result<Object, Error> {
    crate::api::gpu::create_bind_group(bindings)
}

/// Draws a surface's image for this frame.
///
/// # Errors
/// Objects that are not the plugin's or of the right kind, a draw before a
/// pipeline, and more draws than a frame allows.
pub fn render(surface: Object, clear: Option<Color>, commands: &[Command]) -> Result<(), Error> {
    crate::api::gpu::render(surface, clear, commands)
}
