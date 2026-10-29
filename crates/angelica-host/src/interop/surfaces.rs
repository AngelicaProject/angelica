//! Plugins' GPU objects on the game's Direct3D 11 device, and the images
//! of their surfaces. Each frame the overlay runs the operations the `gpu`
//! module queued, in order, on the game's immediate context, with the
//! game's state saved before and put back after, and hands the surfaces'
//! images to the painter, which composites them under Angelica's menu.
//! Direct3D 11 reads past the end of a buffer or a texture as zeros, which
//! keeps a plugin's draw within its own objects.
use super::painter::{SLOTS, Saved, needed};
use crate::gpu::{
    Address, Blend, Command, Filter, Format, Id, Op, Placed, Resource, Topology, VertexFormat,
};
use std::collections::HashMap;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{
                D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_LINELIST,
                D3D_PRIMITIVE_TOPOLOGY_LINESTRIP, D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
                D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
                Fxc::D3DCompile, ID3DBlob,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_INDEX_BUFFER, D3D11_BIND_RENDER_TARGET,
                D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND,
                D3D11_BLEND_DESC, D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD,
                D3D11_BLEND_SRC_ALPHA, D3D11_BLEND_ZERO, D3D11_BOX, D3D11_BUFFER_DESC,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_NEVER, D3D11_CULL_NONE,
                D3D11_FILL_SOLID, D3D11_FILTER, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                D3D11_FILTER_MIN_MAG_MIP_POINT, D3D11_INPUT_ELEMENT_DESC,
                D3D11_INPUT_PER_VERTEX_DATA, D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC,
                D3D11_SAMPLER_DESC, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE_ADDRESS_MIRROR,
                D3D11_TEXTURE_ADDRESS_MODE, D3D11_TEXTURE_ADDRESS_WRAP, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DEFAULT, D3D11_VIEWPORT, ID3D11BlendState, ID3D11Buffer, ID3D11Device,
                ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
                ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView,
                ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::Common::{
                DXGI_FORMAT, DXGI_FORMAT_R8_UNORM, DXGI_FORMAT_R8G8B8A8_UNORM,
                DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R16_UINT, DXGI_FORMAT_R32_FLOAT,
                DXGI_FORMAT_R32_SINT, DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G32_FLOAT,
                DXGI_FORMAT_R32G32B32_FLOAT, DXGI_FORMAT_R32G32B32A32_FLOAT, DXGI_SAMPLE_DESC,
            },
        },
    },
    core::{PCSTR, s},
};

/// Compiles the entry point `entry` of `hlsl` for shader model 5.0.
///
/// # Errors
/// The compiler's message.
pub fn compile(hlsl: &str, entry: &str, vertex: bool) -> Result<Vec<u8>, String> {
    let entry = std::ffi::CString::new(entry).map_err(|e| e.to_string())?;
    let target = if vertex { s!("vs_5_0") } else { s!("ps_5_0") };
    let mut code: Option<ID3DBlob> = None;
    let mut error: Option<ID3DBlob> = None;
    // SAFETY: the source, the entry's name, and the target live for the
    // call; no include handler, so the source reaches nothing else.
    let compiled = unsafe {
        D3DCompile(
            hlsl.as_ptr().cast(),
            hlsl.len(),
            s!("plugin"),
            None,
            None,
            PCSTR(entry.as_ptr().cast()),
            target,
            0,
            0,
            &raw mut code,
            Some(&raw mut error),
        )
    };
    let bytes = |blob: &ID3DBlob| {
        // SAFETY: a blob owns readable bytes for exactly its size.
        unsafe {
            std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
        }
        .to_vec()
    };
    match (compiled, code) {
        (Ok(()), Some(code)) => Ok(bytes(&code)),
        (result, _) => Err(error.as_ref().map_or_else(
            || {
                result
                    .err()
                    .map_or_else(|| "no code".into(), |e| e.to_string())
            },
            |error| String::from_utf8_lossy(&bytes(error)).trim().to_owned(),
        )),
    }
}

struct Buffer {
    buffer: ID3D11Buffer,
    /// What a uniform buffer holds: Direct3D 11 updates a constant buffer
    /// only whole.
    uniform: Option<Vec<u8>>,
}

struct Texture {
    texture: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
}

struct Pipeline {
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    layout: Option<ID3D11InputLayout>,
    stride: u32,
    topology: D3D_PRIMITIVE_TOPOLOGY,
    blend: ID3D11BlendState,
}

struct Surface {
    texture: ID3D11Texture2D,
    target: ID3D11RenderTargetView,
    view: ID3D11ShaderResourceView,
    size: [u32; 2],
}

enum Native {
    Buffer(Buffer),
    Texture(Texture),
    Sampler(ID3D11SamplerState),
    Pipeline(Pipeline),
    BindGroup(Vec<(u32, Resource)>),
    Surface(Surface),
}

/// Plugins' objects on one device.
pub struct Surfaces {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    raster: ID3D11RasterizerState,
    objects: HashMap<Id, Native>,
}

fn dxgi(format: Format) -> DXGI_FORMAT {
    match format {
        Format::Rgba8 => DXGI_FORMAT_R8G8B8A8_UNORM,
        Format::Rgba8Srgb => DXGI_FORMAT_R8G8B8A8_UNORM_SRGB,
        Format::R8 => DXGI_FORMAT_R8_UNORM,
    }
}

fn vertex_format(format: VertexFormat) -> DXGI_FORMAT {
    match format {
        VertexFormat::Float32 => DXGI_FORMAT_R32_FLOAT,
        VertexFormat::Float32x2 => DXGI_FORMAT_R32G32_FLOAT,
        VertexFormat::Float32x3 => DXGI_FORMAT_R32G32B32_FLOAT,
        VertexFormat::Float32x4 => DXGI_FORMAT_R32G32B32A32_FLOAT,
        VertexFormat::Unorm8x4 => DXGI_FORMAT_R8G8B8A8_UNORM,
        VertexFormat::Uint32 => DXGI_FORMAT_R32_UINT,
        VertexFormat::Sint32 => DXGI_FORMAT_R32_SINT,
    }
}

fn topology(topology: Topology) -> D3D_PRIMITIVE_TOPOLOGY {
    match topology {
        Topology::TriangleList => D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST,
        Topology::TriangleStrip => D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP,
        Topology::LineList => D3D_PRIMITIVE_TOPOLOGY_LINELIST,
        Topology::LineStrip => D3D_PRIMITIVE_TOPOLOGY_LINESTRIP,
        Topology::PointList => D3D_PRIMITIVE_TOPOLOGY_POINTLIST,
    }
}

/// The source and destination factors of color and alpha for `blend`.
fn factors(blend: Blend) -> Option<[D3D11_BLEND; 4]> {
    match blend {
        Blend::Replace => None,
        Blend::Alpha => Some([
            D3D11_BLEND_SRC_ALPHA,
            D3D11_BLEND_INV_SRC_ALPHA,
            D3D11_BLEND_ONE,
            D3D11_BLEND_INV_SRC_ALPHA,
        ]),
        Blend::Premultiplied => Some([
            D3D11_BLEND_ONE,
            D3D11_BLEND_INV_SRC_ALPHA,
            D3D11_BLEND_ONE,
            D3D11_BLEND_INV_SRC_ALPHA,
        ]),
        Blend::Additive => Some([
            D3D11_BLEND_ONE,
            D3D11_BLEND_ONE,
            D3D11_BLEND_ONE,
            D3D11_BLEND_ONE,
        ]),
    }
}

/// The names `LOC0` to `LOC7` the translated shaders give vertex inputs.
const LOCATION: PCSTR = s!("LOC");

impl Surfaces {
    /// Plugins' objects on the device of `device` and `context`.
    ///
    /// # Errors
    /// What the device refused.
    pub fn new(device: &ID3D11Device, context: &ID3D11DeviceContext) -> Result<Self, String> {
        let desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            ScissorEnable: true.into(),
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let mut raster = None;
        // SAFETY: a valid descriptor and an owned output.
        unsafe { device.CreateRasterizerState(&raw const desc, Some(&raw mut raster)) }
            .map_err(|e| e.to_string())?;
        Ok(Self {
            device: device.clone(),
            context: context.clone(),
            raster: needed(raster, "raster state")?,
            objects: HashMap::new(),
        })
    }

    /// Whether this is on `device`.
    pub fn serves(&self, device: &ID3D11Device) -> bool {
        windows::core::Interface::as_raw(&self.device) == windows::core::Interface::as_raw(device)
    }

    /// Runs `ops` in order on the immediate context, the game's state saved
    /// and put back, and gives the images of `placed` from the bottom up.
    /// An operation the device refuses is skipped and named in `failed`.
    pub fn run(
        &mut self,
        ops: Vec<Op>,
        placed: &[Placed],
        failed: &mut Vec<String>,
    ) -> Vec<(
        u64,
        ID3D11Texture2D,
        ID3D11ShaderResourceView,
        [usize; 2],
        Placed,
    )> {
        // SAFETY: Present runs on the thread that owns the immediate
        // context; the game's state is read here and put back below.
        let saved = unsafe { Saved::read(&self.context) };
        for op in ops {
            if let Err(error) = self.op(op) {
                failed.push(error);
            }
        }
        // SAFETY: as above, on the same context and thread.
        unsafe { saved.restore(&self.context) };
        placed
            .iter()
            .filter_map(|placed| match self.objects.get(&placed.id) {
                Some(Native::Surface(surface)) => Some((
                    placed.id,
                    surface.texture.clone(),
                    surface.view.clone(),
                    [surface.size[0] as usize, surface.size[1] as usize],
                    placed.clone(),
                )),
                _ => None,
            })
            .collect()
    }

    #[allow(clippy::too_many_lines)]
    fn op(&mut self, op: Op) -> Result<(), String> {
        match op {
            Op::Buffer { id, size, usage } => {
                let bind = if usage.uniform {
                    D3D11_BIND_CONSTANT_BUFFER.0
                } else {
                    (if usage.vertex {
                        D3D11_BIND_VERTEX_BUFFER.0
                    } else {
                        0
                    }) | (if usage.index {
                        D3D11_BIND_INDEX_BUFFER.0
                    } else {
                        0
                    })
                };
                let desc = D3D11_BUFFER_DESC {
                    ByteWidth: size,
                    Usage: D3D11_USAGE_DEFAULT,
                    BindFlags: bind.cast_unsigned(),
                    ..Default::default()
                };
                let mut buffer = None;
                // SAFETY: a valid descriptor and an owned output.
                unsafe {
                    self.device
                        .CreateBuffer(&raw const desc, None, Some(&raw mut buffer))
                }
                .map_err(|e| e.to_string())?;
                self.objects.insert(
                    id,
                    Native::Buffer(Buffer {
                        buffer: needed(buffer, "buffer")?,
                        uniform: usage.uniform.then(|| vec![0; size as usize]),
                    }),
                );
            }
            Op::WriteBuffer { id, offset, bytes } => {
                let Some(Native::Buffer(buffer)) = self.objects.get_mut(&id) else {
                    return Err(format!("no buffer {id}"));
                };
                if let Some(whole) = &mut buffer.uniform {
                    let start = offset as usize;
                    whole[start..start + bytes.len()].copy_from_slice(&bytes);
                    // SAFETY: the buffer is default-usage, written whole
                    // from a slice of its size.
                    unsafe {
                        self.context.UpdateSubresource(
                            &buffer.buffer,
                            0,
                            None,
                            whole.as_ptr().cast(),
                            0,
                            0,
                        );
                    }
                } else {
                    let area = D3D11_BOX {
                        left: offset,
                        top: 0,
                        front: 0,
                        right: offset + u32::try_from(bytes.len()).map_err(|e| e.to_string())?,
                        bottom: 1,
                        back: 1,
                    };
                    // SAFETY: the box lies inside the buffer, which the
                    // `gpu` module checked, and `bytes` holds it.
                    unsafe {
                        self.context.UpdateSubresource(
                            &buffer.buffer,
                            0,
                            Some(&raw const area),
                            bytes.as_ptr().cast(),
                            0,
                            0,
                        );
                    }
                }
            }
            Op::Texture {
                id,
                width,
                height,
                format,
            } => {
                let (texture, view) = self.texture(width, height, dxgi(format), false)?;
                self.objects
                    .insert(id, Native::Texture(Texture { texture, view }));
            }
            Op::WriteTexture {
                id,
                area,
                format,
                bytes,
            } => {
                let Some(Native::Texture(texture)) = self.objects.get(&id) else {
                    return Err(format!("no texture {id}"));
                };
                let x = area.x.cast_unsigned();
                let y = area.y.cast_unsigned();
                let region = D3D11_BOX {
                    left: x,
                    top: y,
                    front: 0,
                    right: x + area.width,
                    bottom: y + area.height,
                    back: 1,
                };
                // SAFETY: the area lies inside the texture and `bytes`
                // holds its rows without gaps, which the `gpu` module
                // checked.
                unsafe {
                    self.context.UpdateSubresource(
                        &texture.texture,
                        0,
                        Some(&raw const region),
                        bytes.as_ptr().cast(),
                        area.width * format.bytes(),
                        0,
                    );
                }
            }
            Op::Sampler {
                id,
                filter,
                address,
            } => {
                let filter: D3D11_FILTER = match filter {
                    Filter::Nearest => D3D11_FILTER_MIN_MAG_MIP_POINT,
                    Filter::Linear => D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                };
                let address: D3D11_TEXTURE_ADDRESS_MODE = match address {
                    Address::Clamp => D3D11_TEXTURE_ADDRESS_CLAMP,
                    Address::Repeat => D3D11_TEXTURE_ADDRESS_WRAP,
                    Address::Mirror => D3D11_TEXTURE_ADDRESS_MIRROR,
                };
                let desc = D3D11_SAMPLER_DESC {
                    Filter: filter,
                    AddressU: address,
                    AddressV: address,
                    AddressW: address,
                    ComparisonFunc: D3D11_COMPARISON_NEVER,
                    MaxLOD: f32::MAX,
                    ..Default::default()
                };
                let mut sampler = None;
                // SAFETY: a valid descriptor and an owned output.
                unsafe {
                    self.device
                        .CreateSamplerState(&raw const desc, Some(&raw mut sampler))
                }
                .map_err(|e| e.to_string())?;
                self.objects
                    .insert(id, Native::Sampler(needed(sampler, "sampler")?));
            }
            Op::Pipeline {
                id,
                vertex,
                fragment,
                stride,
                attributes,
                topology: kind,
                blend,
            } => {
                let pipeline =
                    self.pipeline(&vertex, &fragment, stride, &attributes, kind, blend)?;
                self.objects.insert(id, Native::Pipeline(pipeline));
            }
            Op::BindGroup { id, bindings } => {
                self.objects.insert(id, Native::BindGroup(bindings));
            }
            Op::Surface { id, width, height } => {
                let (texture, view) =
                    self.texture(width, height, DXGI_FORMAT_R8G8B8A8_UNORM, true)?;
                let mut target = None;
                // SAFETY: the texture binds as a render target; an owned
                // output.
                unsafe {
                    self.device
                        .CreateRenderTargetView(&texture, None, Some(&raw mut target))
                }
                .map_err(|e| e.to_string())?;
                let target = needed(target, "render target")?;
                // SAFETY: a view of a texture of this device.
                unsafe { self.context.ClearRenderTargetView(&target, &[0.0; 4]) };
                self.objects.insert(
                    id,
                    Native::Surface(Surface {
                        texture,
                        target,
                        view,
                        size: [width, height],
                    }),
                );
            }
            Op::Render {
                surface,
                clear,
                commands,
            } => self.render(surface, clear, &commands)?,
            Op::Destroy(id) => {
                self.objects.remove(&id);
            }
        }
        Ok(())
    }

    fn texture(
        &self,
        width: u32,
        height: u32,
        format: DXGI_FORMAT,
        target: bool,
    ) -> Result<(ID3D11Texture2D, ID3D11ShaderResourceView), String> {
        let bind = D3D11_BIND_SHADER_RESOURCE.0
            | if target {
                D3D11_BIND_RENDER_TARGET.0
            } else {
                0
            };
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: format,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: bind.cast_unsigned(),
            ..Default::default()
        };
        let mut texture = None;
        let mut view = None;
        // SAFETY: a valid descriptor and owned outputs; the default view
        // matches the texture.
        unsafe {
            self.device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut texture))
                .map_err(|e| e.to_string())?;
            let texture = needed(texture.clone(), "texture")?;
            self.device
                .CreateShaderResourceView(&texture, None, Some(&raw mut view))
                .map_err(|e| e.to_string())?;
        }
        Ok((needed(texture, "texture")?, needed(view, "texture view")?))
    }

    fn pipeline(
        &self,
        vertex: &[u8],
        fragment: &[u8],
        stride: u32,
        attributes: &[crate::gpu::Attribute],
        kind: Topology,
        blend: Blend,
    ) -> Result<Pipeline, String> {
        let (mut vs, mut ps, mut layout, mut state) = (None, None, None, None);
        let input: Vec<D3D11_INPUT_ELEMENT_DESC> = attributes
            .iter()
            .map(|attribute| D3D11_INPUT_ELEMENT_DESC {
                SemanticName: LOCATION,
                SemanticIndex: attribute.location,
                Format: vertex_format(attribute.format),
                InputSlot: 0,
                AlignedByteOffset: attribute.offset,
                InputSlotClass: D3D11_INPUT_PER_VERTEX_DATA,
                InstanceDataStepRate: 0,
            })
            .collect();
        let mut blend_desc = D3D11_BLEND_DESC::default();
        let write = u8::try_from(D3D11_COLOR_WRITE_ENABLE_ALL.0).map_err(|e| e.to_string())?;
        blend_desc.RenderTarget[0] = match factors(blend) {
            None => D3D11_RENDER_TARGET_BLEND_DESC {
                BlendEnable: false.into(),
                SrcBlend: D3D11_BLEND_ONE,
                DestBlend: D3D11_BLEND_ZERO,
                BlendOp: D3D11_BLEND_OP_ADD,
                SrcBlendAlpha: D3D11_BLEND_ONE,
                DestBlendAlpha: D3D11_BLEND_ZERO,
                BlendOpAlpha: D3D11_BLEND_OP_ADD,
                RenderTargetWriteMask: write,
            },
            Some([source, destination, source_alpha, destination_alpha]) => {
                D3D11_RENDER_TARGET_BLEND_DESC {
                    BlendEnable: true.into(),
                    SrcBlend: source,
                    DestBlend: destination,
                    BlendOp: D3D11_BLEND_OP_ADD,
                    SrcBlendAlpha: source_alpha,
                    DestBlendAlpha: destination_alpha,
                    BlendOpAlpha: D3D11_BLEND_OP_ADD,
                    RenderTargetWriteMask: write,
                }
            }
        };
        // SAFETY: the bytecode came from the compiler for these stages, the
        // input layout matches the vertex shader's inputs, which the `gpu`
        // module checked, and every output is owned.
        unsafe {
            self.device
                .CreateVertexShader(vertex, None, Some(&raw mut vs))
                .map_err(|e| e.to_string())?;
            self.device
                .CreatePixelShader(fragment, None, Some(&raw mut ps))
                .map_err(|e| e.to_string())?;
            if !input.is_empty() {
                self.device
                    .CreateInputLayout(&input, vertex, Some(&raw mut layout))
                    .map_err(|e| e.to_string())?;
            }
            self.device
                .CreateBlendState(&raw const blend_desc, Some(&raw mut state))
                .map_err(|e| e.to_string())?;
        }
        Ok(Pipeline {
            vertex: needed(vs, "vertex shader")?,
            pixel: needed(ps, "pixel shader")?,
            layout,
            stride,
            topology: topology(kind),
            blend: needed(state, "blend state")?,
        })
    }

    #[allow(clippy::too_many_lines)]
    fn render(
        &self,
        surface: Id,
        clear: Option<[f32; 4]>,
        commands: &[Command],
    ) -> Result<(), String> {
        let Some(Native::Surface(target)) = self.objects.get(&surface) else {
            return Err(format!("no surface {surface}"));
        };
        let context = &self.context;
        let [width, height] = target.size;
        #[allow(clippy::cast_precision_loss)]
        let viewport = D3D11_VIEWPORT {
            TopLeftX: 0.0,
            TopLeftY: 0.0,
            Width: width as f32,
            Height: height as f32,
            MinDepth: 0.0,
            MaxDepth: 1.0,
        };
        let whole = RECT {
            left: 0,
            top: 0,
            right: i32::try_from(width).map_err(|e| e.to_string())?,
            bottom: i32::try_from(height).map_err(|e| e.to_string())?,
        };
        // SAFETY: Present's thread owns the immediate context; every object
        // set belongs to this device and lives while it is set.
        unsafe {
            // A surface's image may be bound for reading: not while it is
            // the target.
            context.PSSetShaderResources(0, Some(&[const { None }; SLOTS]));
            context.VSSetShaderResources(0, Some(&[const { None }; SLOTS]));
            context.OMSetRenderTargets(Some(&[Some(target.target.clone())]), None);
            context.OMSetDepthStencilState(None, 0);
            context.RSSetState(&self.raster);
            context.RSSetViewports(Some(&[viewport]));
            context.RSSetScissorRects(Some(&[whole]));
            context.GSSetShader(None, None);
            context.HSSetShader(None, None);
            context.DSSetShader(None, None);
            if let Some(color) = clear {
                // The surface holds premultiplied colors.
                let [r, g, b, a] = color;
                context.ClearRenderTargetView(&target.target, &[r * a, g * a, b * a, a]);
            }
        }
        let mut stride = 0;
        let mut vertex: Option<(ID3D11Buffer, u32)> = None;
        let mut ready = false;
        for command in commands {
            // SAFETY: as above.
            unsafe {
                match command {
                    Command::Pipeline(id) => {
                        let Some(Native::Pipeline(pipeline)) = self.objects.get(id) else {
                            ready = false;
                            continue;
                        };
                        context.VSSetShader(&pipeline.vertex, None);
                        context.PSSetShader(&pipeline.pixel, None);
                        context.IASetInputLayout(pipeline.layout.as_ref());
                        context.IASetPrimitiveTopology(pipeline.topology);
                        context.OMSetBlendState(&pipeline.blend, None, u32::MAX);
                        stride = pipeline.stride;
                        ready = true;
                        if let Some((buffer, offset)) = &vertex {
                            let set = Some(buffer.clone());
                            context.IASetVertexBuffers(
                                0,
                                1,
                                Some(&raw const set),
                                Some(&raw const stride),
                                Some(&raw const *offset),
                            );
                        }
                    }
                    Command::BindGroup(id) => {
                        let Some(Native::BindGroup(bindings)) = self.objects.get(id) else {
                            continue;
                        };
                        for (index, resource) in bindings {
                            self.bind(*index, *resource);
                        }
                    }
                    Command::VertexBuffer { buffer, offset } => {
                        let Some(Native::Buffer(found)) = self.objects.get(buffer) else {
                            continue;
                        };
                        let set = Some(found.buffer.clone());
                        context.IASetVertexBuffers(
                            0,
                            1,
                            Some(&raw const set),
                            Some(&raw const stride),
                            Some(offset),
                        );
                        vertex = Some((found.buffer.clone(), *offset));
                    }
                    Command::IndexBuffer {
                        buffer,
                        offset,
                        wide,
                    } => {
                        let Some(Native::Buffer(found)) = self.objects.get(buffer) else {
                            continue;
                        };
                        let format = if *wide {
                            DXGI_FORMAT_R32_UINT
                        } else {
                            DXGI_FORMAT_R16_UINT
                        };
                        context.IASetIndexBuffer(&found.buffer, format, *offset);
                    }
                    Command::Scissor(area) => {
                        let rect = RECT {
                            left: area.x.max(0),
                            top: area.y.max(0),
                            right: area
                                .x
                                .saturating_add(i32::try_from(area.width).unwrap_or(i32::MAX))
                                .min(whole.right),
                            bottom: area
                                .y
                                .saturating_add(i32::try_from(area.height).unwrap_or(i32::MAX))
                                .min(whole.bottom),
                        };
                        context.RSSetScissorRects(Some(&[rect]));
                    }
                    Command::Draw {
                        vertices,
                        instances,
                        first_vertex,
                        first_instance,
                    } if ready => {
                        context.DrawInstanced(
                            *vertices,
                            *instances,
                            *first_vertex,
                            *first_instance,
                        );
                    }
                    Command::DrawIndexed {
                        indices,
                        instances,
                        first_index,
                        base_vertex,
                        first_instance,
                    } if ready => {
                        context.DrawIndexedInstanced(
                            *indices,
                            *instances,
                            *first_index,
                            *base_vertex,
                            *first_instance,
                        );
                    }
                    Command::Draw { .. } | Command::DrawIndexed { .. } => {}
                }
            }
        }
        Ok(())
    }

    /// Binds `resource` at `index` for both stages.
    ///
    /// # Safety
    /// On Present's thread, which owns the immediate context.
    unsafe fn bind(&self, index: u32, resource: Resource) {
        let context = &self.context;
        // SAFETY: the caller's; every object belongs to this device.
        unsafe {
            match resource {
                Resource::Uniform(id) => {
                    if let Some(Native::Buffer(found)) = self.objects.get(&id) {
                        let set = [Some(found.buffer.clone())];
                        context.VSSetConstantBuffers(index, Some(&set));
                        context.PSSetConstantBuffers(index, Some(&set));
                    }
                }
                Resource::Texture(id) => {
                    if let Some(Native::Texture(found)) = self.objects.get(&id) {
                        let set = [Some(found.view.clone())];
                        context.VSSetShaderResources(index, Some(&set));
                        context.PSSetShaderResources(index, Some(&set));
                    }
                }
                Resource::Sampler(id) => {
                    if let Some(Native::Sampler(found)) = self.objects.get(&id) {
                        let set = [Some(found.clone())];
                        context.VSSetSamplers(index, Some(&set));
                        context.PSSetSamplers(index, Some(&set));
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gpu::{Attribute, Gpu, Local, Rect, Usage};
    use windows::Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_WARP,
        Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_FLAG, D3D11_MAP_READ,
            D3D11_MAPPED_SUBRESOURCE, D3D11_SDK_VERSION, D3D11_USAGE_STAGING, D3D11CreateDevice,
        },
    };

    /// A triangle that covers the whole surface, in the color of a uniform.
    const SHADER: &str = r"
struct Tint { color: vec4<f32> };
@group(0) @binding(0) var<uniform> tint: Tint;
@vertex fn vs(@location(0) pos: vec2<f32>) -> @builtin(position) vec4<f32> {
    return vec4<f32>(pos, 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> {
    return tint.color;
}
";

    fn warp() -> (ID3D11Device, ID3D11DeviceContext) {
        let (mut device, mut context) = (None, None);
        // SAFETY: a software device with owned outputs.
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_WARP,
                windows::Win32::Foundation::HMODULE::default(),
                D3D11_CREATE_DEVICE_FLAG(0),
                None,
                D3D11_SDK_VERSION,
                Some(&raw mut device),
                None,
                Some(&raw mut context),
            )
        }
        .unwrap();
        (device.unwrap(), context.unwrap())
    }

    /// The texel at the top left of `texture`, of RGBA bytes.
    fn first_texel(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        texture: &ID3D11Texture2D,
    ) -> [u8; 4] {
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: a live texture of the device, read through a staging copy.
        unsafe {
            texture.GetDesc(&raw mut desc);
            desc.Usage = D3D11_USAGE_STAGING;
            desc.BindFlags = 0;
            desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0.cast_unsigned();
            let mut staging = None;
            device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut staging))
                .unwrap();
            let staging = staging.unwrap();
            context.CopyResource(&staging, texture);
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
                .unwrap();
            let texel = std::slice::from_raw_parts(mapped.pData.cast::<u8>(), 4);
            let texel = [texel[0], texel[1], texel[2], texel[3]];
            context.Unmap(&staging, 0);
            texel
        }
    }

    /// A surface of 8 by 8 pixels of `gpu`'s plugin `one`, rendered blue
    /// and then with a red triangle over all of it.
    fn red_triangle(gpu: &Gpu) -> u32 {
        let surface = gpu
            .create_surface(
                "one",
                Rect {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 8,
                },
            )
            .unwrap();
        let shader = gpu.create_shader("one", SHADER).unwrap();
        let pipeline = gpu
            .create_pipeline(
                "one",
                shader,
                "vs",
                "fs",
                8,
                vec![Attribute {
                    location: 0,
                    format: crate::gpu::VertexFormat::Float32x2,
                    offset: 0,
                }],
                Topology::TriangleList,
                Blend::Replace,
                &compile,
            )
            .unwrap();
        let vertices = gpu
            .create_buffer(
                "one",
                24,
                Usage {
                    vertex: true,
                    index: false,
                    uniform: false,
                },
            )
            .unwrap();
        let triangle: Vec<u8> = [-1.0f32, -1.0, -1.0, 3.0, 3.0, -1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        gpu.write_buffer("one", vertices, 0, triangle).unwrap();
        let tint = gpu
            .create_buffer(
                "one",
                16,
                Usage {
                    vertex: false,
                    index: false,
                    uniform: true,
                },
            )
            .unwrap();
        let red: Vec<u8> = [1.0f32, 0.0, 0.0, 1.0]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        gpu.write_buffer("one", tint, 0, red).unwrap();
        let group = gpu.create_bind_group("one", &[(0, tint)]).unwrap();
        gpu.render(
            "one",
            surface,
            Some([0.0, 0.0, 1.0, 1.0]),
            vec![
                Local::Pipeline(pipeline),
                Local::BindGroup(group),
                Local::VertexBuffer {
                    buffer: vertices,
                    offset: 0,
                },
                Local::Draw(Command::Draw {
                    vertices: 3,
                    instances: 1,
                    first_vertex: 0,
                    first_instance: 0,
                }),
            ],
        )
        .unwrap();
        surface
    }

    #[test]
    fn a_plugins_wgsl_draws_on_its_surface_through_direct3d_11() {
        let (device, context) = warp();
        let gpu = Gpu::default();
        let surface = red_triangle(&gpu);
        let mut surfaces = Surfaces::new(&device, &context).unwrap();
        let frame = gpu.take();
        let mut failed = Vec::new();
        let images = surfaces.run(frame.ops, &frame.placed, &mut failed);
        assert!(failed.is_empty(), "{failed:?}");
        assert_eq!(images.len(), 1);
        assert_eq!(
            first_texel(&device, &context, &images[0].1),
            [255, 0, 0, 255]
        );

        // A clear without draws leaves the clear color, premultiplied.
        gpu.render("one", surface, Some([0.0, 1.0, 0.0, 0.5]), Vec::new())
            .unwrap();
        let frame = gpu.take();
        let images = surfaces.run(frame.ops, &frame.placed, &mut failed);
        assert_eq!(
            first_texel(&device, &context, &images[0].1),
            [0, 128, 0, 128]
        );
    }

    #[test]
    fn a_shader_the_compiler_refuses_names_the_reason() {
        let error = compile("float4 broken( : SV_Target { }", "broken", false).unwrap_err();
        assert!(!error.is_empty());
    }
}
