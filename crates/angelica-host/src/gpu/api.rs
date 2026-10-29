//! The `gpu` and `look` interfaces of the main instance.
use super::{
    Address, Attribute, Blend, Command, Filter, Format, Local, Rect, Topology, Usage, VertexFormat,
};
use crate::error::{Error, Kind, Result};
use crate::sandbox::State;
use crate::sandbox::bindings::angelica::plugin::{gpu as api, look as look_api};

fn rect(area: api::Rect) -> Rect {
    Rect {
        x: area.x,
        y: area.y,
        width: area.width,
        height: area.height,
    }
}

impl State {
    /// Runs `work` with the GPU service and the plugin's id, in the main
    /// instance.
    fn gpu<T>(&mut self, work: impl FnOnce(&super::Gpu, &str) -> Result<T>) -> Result<T> {
        self.host(|state| {
            if !matches!(state.role, crate::sandbox::Role::Main) {
                return Err(Error::new(
                    Kind::WrongContext,
                    "only the main instance draws",
                ));
            }
            let context = state.context.clone();
            work(&context.services.gpu, &context.id)
        })
    }
}

impl api::Host for State {
    fn screen(&mut self) -> api::Size {
        let (width, height) = self.context.services.gpu.screen();
        api::Size { width, height }
    }

    fn camera(&mut self) -> Option<Vec<f32>> {
        self.context
            .services
            .gpu
            .camera()
            .map(|matrix| matrix.to_vec())
    }

    fn create_surface(&mut self, area: api::Rect) -> std::result::Result<api::Object, api::Error> {
        self.gpu(|gpu, plugin| gpu.create_surface(plugin, rect(area)))
            .map_err(Into::into)
    }

    fn set_surface(
        &mut self,
        surface: api::Object,
        area: api::Rect,
        opacity: f32,
        input: Vec<api::Rect>,
        keyboard: bool,
    ) -> std::result::Result<(), api::Error> {
        self.gpu(|gpu, plugin| {
            gpu.set_surface(
                plugin,
                surface,
                rect(area),
                opacity,
                input.into_iter().map(rect).collect(),
                keyboard,
            )
        })
        .map_err(Into::into)
    }

    fn focused(&mut self) -> Option<api::Object> {
        let context = self.context.clone();
        context.services.gpu.focused(&context.id)
    }

    fn destroy(&mut self, object: api::Object) {
        let _ = self.gpu(|gpu, plugin| {
            gpu.destroy(plugin, object);
            Ok(())
        });
    }

    fn create_buffer(
        &mut self,
        size: u32,
        usage: api::BufferUsage,
    ) -> std::result::Result<api::Object, api::Error> {
        let usage = Usage {
            vertex: usage.contains(api::BufferUsage::VERTEX),
            index: usage.contains(api::BufferUsage::INDEX),
            uniform: usage.contains(api::BufferUsage::UNIFORM),
        };
        self.gpu(|gpu, plugin| gpu.create_buffer(plugin, size, usage))
            .map_err(Into::into)
    }

    fn write_buffer(
        &mut self,
        buffer: api::Object,
        offset: u32,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), api::Error> {
        self.gpu(|gpu, plugin| gpu.write_buffer(plugin, buffer, offset, bytes))
            .map_err(Into::into)
    }

    fn create_texture(
        &mut self,
        width: u32,
        height: u32,
        format: api::TextureFormat,
    ) -> std::result::Result<api::Object, api::Error> {
        let format = match format {
            api::TextureFormat::Rgba8Unorm => Format::Rgba8,
            api::TextureFormat::Rgba8UnormSrgb => Format::Rgba8Srgb,
            api::TextureFormat::R8Unorm => Format::R8,
        };
        self.gpu(|gpu, plugin| gpu.create_texture(plugin, width, height, format))
            .map_err(Into::into)
    }

    fn write_texture(
        &mut self,
        texture: api::Object,
        area: api::Rect,
        bytes: Vec<u8>,
    ) -> std::result::Result<(), api::Error> {
        self.gpu(|gpu, plugin| gpu.write_texture(plugin, texture, rect(area), bytes))
            .map_err(Into::into)
    }

    fn create_sampler(
        &mut self,
        filter: api::Filter,
        address: api::AddressMode,
    ) -> std::result::Result<api::Object, api::Error> {
        let filter = match filter {
            api::Filter::Nearest => Filter::Nearest,
            api::Filter::Linear => Filter::Linear,
        };
        let address = match address {
            api::AddressMode::Clamp => Address::Clamp,
            api::AddressMode::Repeat => Address::Repeat,
            api::AddressMode::Mirror => Address::Mirror,
        };
        self.gpu(|gpu, plugin| gpu.create_sampler(plugin, filter, address))
            .map_err(Into::into)
    }

    fn create_shader(&mut self, wgsl: String) -> std::result::Result<api::Object, api::Error> {
        self.gpu(|gpu, plugin| gpu.create_shader(plugin, &wgsl))
            .map_err(Into::into)
    }

    fn create_pipeline(
        &mut self,
        desc: api::PipelineDesc,
    ) -> std::result::Result<api::Object, api::Error> {
        let attributes = desc
            .attributes
            .iter()
            .map(|attribute| Attribute {
                location: attribute.location,
                format: match attribute.format {
                    api::VertexFormat::Float32 => VertexFormat::Float32,
                    api::VertexFormat::Float32x2 => VertexFormat::Float32x2,
                    api::VertexFormat::Float32x3 => VertexFormat::Float32x3,
                    api::VertexFormat::Float32x4 => VertexFormat::Float32x4,
                    api::VertexFormat::Unorm8x4 => VertexFormat::Unorm8x4,
                    api::VertexFormat::Uint32 => VertexFormat::Uint32,
                    api::VertexFormat::Sint32 => VertexFormat::Sint32,
                },
                offset: attribute.offset,
            })
            .collect();
        let topology = match desc.topology {
            api::Topology::TriangleList => Topology::TriangleList,
            api::Topology::TriangleStrip => Topology::TriangleStrip,
            api::Topology::LineList => Topology::LineList,
            api::Topology::LineStrip => Topology::LineStrip,
            api::Topology::PointList => Topology::PointList,
        };
        let blend = match desc.blend {
            api::Blend::Replace => Blend::Replace,
            api::Blend::Alpha => Blend::Alpha,
            api::Blend::Premultiplied => Blend::Premultiplied,
            api::Blend::Additive => Blend::Additive,
        };
        let game = self.context.services.game.clone();
        self.gpu(|gpu, plugin| {
            gpu.create_pipeline(
                plugin,
                desc.shader,
                &desc.vertex_entry,
                &desc.fragment_entry,
                desc.stride,
                attributes,
                topology,
                blend,
                &|hlsl, entry, vertex| game.compile_shader(hlsl, entry, vertex),
            )
        })
        .map_err(Into::into)
    }

    fn create_bind_group(
        &mut self,
        bindings: Vec<api::Binding>,
    ) -> std::result::Result<api::Object, api::Error> {
        let bindings: Vec<(u32, u32)> = bindings
            .iter()
            .map(|binding| (binding.index, binding.bound))
            .collect();
        self.gpu(|gpu, plugin| gpu.create_bind_group(plugin, &bindings))
            .map_err(Into::into)
    }

    fn render(
        &mut self,
        surface: api::Object,
        clear: Option<api::Color>,
        commands: Vec<api::Command>,
    ) -> std::result::Result<(), api::Error> {
        let commands = commands
            .into_iter()
            .map(|command| match command {
                api::Command::Pipeline(object) => Local::Pipeline(object),
                api::Command::BindGroup(object) => Local::BindGroup(object),
                api::Command::VertexBuffer(buffer) => Local::VertexBuffer {
                    buffer: buffer.buffer,
                    offset: buffer.offset,
                },
                api::Command::IndexBuffer(buffer) => Local::IndexBuffer {
                    buffer: buffer.buffer,
                    offset: buffer.offset,
                    wide: buffer.wide,
                },
                api::Command::Scissor(area) => Local::Scissor(rect(area)),
                api::Command::Draw(draw) => Local::Draw(Command::Draw {
                    vertices: draw.vertices,
                    instances: draw.instances,
                    first_vertex: draw.first_vertex,
                    first_instance: draw.first_instance,
                }),
                api::Command::DrawIndexed(draw) => Local::Draw(Command::DrawIndexed {
                    indices: draw.indices,
                    instances: draw.instances,
                    first_index: draw.first_index,
                    base_vertex: draw.base_vertex,
                    first_instance: draw.first_instance,
                }),
            })
            .collect();
        let clear = clear.map(|color| [color.r, color.g, color.b, color.a]);
        self.gpu(|gpu, plugin| gpu.render(plugin, surface, clear, commands))
            .map_err(Into::into)
    }
}

impl look_api::Host for State {
    fn settings(&mut self) -> look_api::LookSettings {
        let services = &self.context.services;
        let (dark, scale) = services.gpu.look();
        look_api::LookSettings {
            dark,
            scale,
            russian: services.russian.load(std::sync::atomic::Ordering::Acquire),
        }
    }

    fn font(&mut self, name: String) -> Option<Vec<u8>> {
        crate::fonts::FACES
            .iter()
            .find(|(face, _)| *face == name)
            .map(|(_, bytes)| bytes.to_vec())
    }
}
