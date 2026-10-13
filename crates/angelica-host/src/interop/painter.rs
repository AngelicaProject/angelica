//! Egui triangles drawn into the game's back buffer on its immediate
//! context, the way proven overlays draw: everything the overlay changes in
//! the context is saved before and put back after, and vertices and indices
//! go into two buffers that are made once and grow.
use egui::{
    TextureId,
    epaint::{ClippedPrimitive, Primitive},
};
use num_traits::ToPrimitive;
use std::collections::HashMap;
use windows::{
    Win32::{
        Foundation::RECT,
        Graphics::{
            Direct3D::{
                D3D_PRIMITIVE_TOPOLOGY, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile,
                ID3DBlob,
            },
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_FLAG, D3D11_BIND_INDEX_BUFFER,
                D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC,
                D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS, D3D11_CPU_ACCESS_WRITE,
                D3D11_CULL_NONE, D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR,
                D3D11_INPUT_ELEMENT_DESC, D3D11_MAP_WRITE_DISCARD, D3D11_MAPPED_SUBRESOURCE,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC,
                D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_DYNAMIC, D3D11_USAGE_IMMUTABLE, D3D11_VIEWPORT, ID3D11BlendState,
                ID3D11Buffer, ID3D11ClassInstance, ID3D11DepthStencilState, ID3D11DepthStencilView,
                ID3D11Device, ID3D11DeviceContext, ID3D11DomainShader, ID3D11GeometryShader,
                ID3D11HullShader, ID3D11InputLayout, ID3D11PixelShader, ID3D11RasterizerState,
                ID3D11RenderTargetView, ID3D11SamplerState, ID3D11ShaderResourceView,
                ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT, DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM,
                    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R16G16B16A16_FLOAT,
                    DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G32_FLOAT, DXGI_SAMPLE_DESC,
                },
                IDXGIDevice, IDXGISwapChain,
            },
        },
    },
    core::{Interface, s},
};

const SHADER: &str = r"
cbuffer Screen : register(b0) { float2 screen; float linear_target; float padding; };
struct Input { float2 pos : POSITION; float2 uv : TEXCOORD0; float4 col : COLOR0; };
struct Output { float4 pos : SV_POSITION; float2 uv : TEXCOORD0; float4 col : COLOR0; };
Output vertex(Input v) { Output o; o.pos=float4(2*v.pos.x/screen.x-1,1-2*v.pos.y/screen.y,0,1); o.uv=v.uv; o.col=v.col; return o; }
Texture2D image : register(t0); SamplerState sampler0 : register(s0);
float3 linearize(float3 s) { return lerp(s/12.92,pow((s+0.055)/1.055,2.4),step(0.04045,s)); }
float4 pixel(Output v) : SV_TARGET { float4 c=v.col*image.Sample(sampler0,v.uv); if(linear_target!=0) c.rgb=linearize(c.rgb); return c; }
";

struct Texture {
    size: [usize; 2],
    bytes: Vec<u8>,
    view: ID3D11ShaderResourceView,
}

/// A buffer the CPU rewrites every frame, made again larger when a frame
/// needs more room.
struct Dynamic {
    bind: D3D11_BIND_FLAG,
    buffer: Option<ID3D11Buffer>,
    capacity: usize,
}

impl Dynamic {
    fn new(bind: D3D11_BIND_FLAG) -> Self {
        Self {
            bind,
            buffer: None,
            capacity: 0,
        }
    }

    /// The buffer, with room for `bytes`.
    fn with_room(&mut self, device: &ID3D11Device, bytes: usize) -> Result<ID3D11Buffer, String> {
        if let Some(buffer) = &self.buffer
            && self.capacity >= bytes
        {
            return Ok(buffer.clone());
        }
        let capacity = bytes.next_power_of_two().max(64 * 1024);
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(capacity).map_err(|e| e.to_string())?,
            Usage: D3D11_USAGE_DYNAMIC,
            BindFlags: self.bind.0.cast_unsigned(),
            CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0.cast_unsigned(),
            ..Default::default()
        };
        let mut buffer = None;
        // SAFETY: a valid dynamic buffer descriptor and an owned output.
        unsafe { device.CreateBuffer(&raw const desc, None, Some(&raw mut buffer)) }
            .map_err(|e| e.to_string())?;
        let buffer = needed(buffer, "buffer")?;
        self.buffer = Some(buffer.clone());
        self.capacity = capacity;
        Ok(buffer)
    }
}

/// Writes `bytes` at the start of the dynamic `buffer`, discarding what it
/// held.
fn write(context: &ID3D11DeviceContext, buffer: &ID3D11Buffer, bytes: &[u8]) -> Result<(), String> {
    let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
    // SAFETY: the buffer is dynamic with CPU write access and has room for
    // `bytes`; the mapping is written once and unmapped before any draw.
    unsafe {
        context
            .Map(buffer, 0, D3D11_MAP_WRITE_DISCARD, 0, Some(&raw mut mapped))
            .map_err(|e| e.to_string())?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.pData.cast::<u8>(), bytes.len());
        context.Unmap(buffer, 0);
    }
    Ok(())
}

/// Shader class instances a stage can hold, the most D3D11 allows.
const INSTANCES: usize = 256;

/// Everything the overlay sets in the game's immediate context, saved before
/// it draws and put back after.
struct Saved {
    scissors: [RECT; 16],
    scissor_count: u32,
    viewports: [D3D11_VIEWPORT; 16],
    viewport_count: u32,
    raster: Option<ID3D11RasterizerState>,
    blend: Option<ID3D11BlendState>,
    blend_factor: [f32; 4],
    sample_mask: u32,
    depth: Option<ID3D11DepthStencilState>,
    stencil: u32,
    resources: [Option<ID3D11ShaderResourceView>; 1],
    samplers: [Option<ID3D11SamplerState>; 1],
    vs_constants: [Option<ID3D11Buffer>; 1],
    ps_constants: [Option<ID3D11Buffer>; 1],
    pixel: (Option<ID3D11PixelShader>, Vec<Option<ID3D11ClassInstance>>),
    vertex: (Option<ID3D11VertexShader>, Vec<Option<ID3D11ClassInstance>>),
    geometry: (
        Option<ID3D11GeometryShader>,
        Vec<Option<ID3D11ClassInstance>>,
    ),
    hull: (Option<ID3D11HullShader>, Vec<Option<ID3D11ClassInstance>>),
    domain: (Option<ID3D11DomainShader>, Vec<Option<ID3D11ClassInstance>>),
    topology: D3D_PRIMITIVE_TOPOLOGY,
    index: Option<ID3D11Buffer>,
    index_format: DXGI_FORMAT,
    index_offset: u32,
    vertex_buffer: Option<ID3D11Buffer>,
    vertex_stride: u32,
    vertex_offset: u32,
    layout: Option<ID3D11InputLayout>,
    targets: [Option<ID3D11RenderTargetView>; 8],
    depth_view: Option<ID3D11DepthStencilView>,
}

/// A stage's shader and its class instances, read with `get`.
fn shader_of<T>(
    get: impl FnOnce(*mut Option<T>, Option<*mut Option<ID3D11ClassInstance>>, Option<*mut u32>),
) -> (Option<T>, Vec<Option<ID3D11ClassInstance>>) {
    let mut shader = None;
    let mut instances = vec![None; INSTANCES];
    let mut count = u32::try_from(INSTANCES).unwrap_or(0);
    get(
        &raw mut shader,
        Some(instances.as_mut_ptr()),
        Some(&raw mut count),
    );
    instances.truncate(usize::try_from(count).unwrap_or(0).min(INSTANCES));
    (shader, instances)
}

impl Saved {
    /// Reads the state of `context`.
    ///
    /// # Safety
    /// `context` is the device's immediate context, used on this thread.
    unsafe fn read(context: &ID3D11DeviceContext) -> Self {
        // SAFETY: every output matches the getter's documented size and is
        // owned by this function; the caller owns the context's thread.
        unsafe {
            let mut saved = Self {
                scissors: Default::default(),
                scissor_count: 16,
                viewports: Default::default(),
                viewport_count: 16,
                raster: context.RSGetState().ok(),
                blend: None,
                blend_factor: [0.0; 4],
                sample_mask: 0,
                depth: None,
                stencil: 0,
                resources: Default::default(),
                samplers: Default::default(),
                vs_constants: Default::default(),
                ps_constants: Default::default(),
                pixel: shader_of(|s, i, n| context.PSGetShader(s, i, n)),
                vertex: shader_of(|s, i, n| context.VSGetShader(s, i, n)),
                geometry: shader_of(|s, i, n| context.GSGetShader(s, i, n)),
                hull: shader_of(|s, i, n| context.HSGetShader(s, i, n)),
                domain: shader_of(|s, i, n| context.DSGetShader(s, i, n)),
                topology: context.IAGetPrimitiveTopology(),
                index: None,
                index_format: DXGI_FORMAT::default(),
                index_offset: 0,
                vertex_buffer: None,
                vertex_stride: 0,
                vertex_offset: 0,
                layout: context.IAGetInputLayout().ok(),
                targets: Default::default(),
                depth_view: None,
            };
            context.RSGetScissorRects(
                &raw mut saved.scissor_count,
                Some(saved.scissors.as_mut_ptr()),
            );
            context.RSGetViewports(
                &raw mut saved.viewport_count,
                Some(saved.viewports.as_mut_ptr()),
            );
            context.OMGetBlendState(
                Some(&raw mut saved.blend),
                Some(&mut saved.blend_factor),
                Some(&raw mut saved.sample_mask),
            );
            context
                .OMGetDepthStencilState(Some(&raw mut saved.depth), Some(&raw mut saved.stencil));
            context.PSGetShaderResources(0, Some(&mut saved.resources));
            context.PSGetSamplers(0, Some(&mut saved.samplers));
            context.VSGetConstantBuffers(0, Some(&mut saved.vs_constants));
            context.PSGetConstantBuffers(0, Some(&mut saved.ps_constants));
            context.IAGetIndexBuffer(
                Some(&raw mut saved.index),
                Some(&raw mut saved.index_format),
                Some(&raw mut saved.index_offset),
            );
            context.IAGetVertexBuffers(
                0,
                1,
                Some(&raw mut saved.vertex_buffer),
                Some(&raw mut saved.vertex_stride),
                Some(&raw mut saved.vertex_offset),
            );
            context.OMGetRenderTargets(Some(&mut saved.targets), Some(&raw mut saved.depth_view));
            saved
        }
    }

    /// Puts the saved state back into `context`.
    ///
    /// # Safety
    /// As for [`Saved::read`], on the context it was read from.
    unsafe fn restore(&self, context: &ID3D11DeviceContext) {
        let scissors = &self.scissors[..(self.scissor_count as usize).min(16)];
        let viewports = &self.viewports[..(self.viewport_count as usize).min(16)];
        // SAFETY: every input was read from this context and is alive.
        unsafe {
            context.RSSetScissorRects(Some(scissors));
            context.RSSetViewports(Some(viewports));
            context.RSSetState(self.raster.as_ref());
            context.OMSetBlendState(
                self.blend.as_ref(),
                Some(&self.blend_factor),
                self.sample_mask,
            );
            context.OMSetDepthStencilState(self.depth.as_ref(), self.stencil);
            context.PSSetShaderResources(0, Some(&self.resources));
            context.PSSetSamplers(0, Some(&self.samplers));
            context.VSSetConstantBuffers(0, Some(&self.vs_constants));
            context.PSSetConstantBuffers(0, Some(&self.ps_constants));
            context.PSSetShader(self.pixel.0.as_ref(), Some(&self.pixel.1));
            context.VSSetShader(self.vertex.0.as_ref(), Some(&self.vertex.1));
            context.GSSetShader(self.geometry.0.as_ref(), Some(&self.geometry.1));
            context.HSSetShader(self.hull.0.as_ref(), Some(&self.hull.1));
            context.DSSetShader(self.domain.0.as_ref(), Some(&self.domain.1));
            context.IASetPrimitiveTopology(self.topology);
            context.IASetIndexBuffer(self.index.as_ref(), self.index_format, self.index_offset);
            context.IASetVertexBuffers(
                0,
                1,
                Some(&raw const self.vertex_buffer),
                Some(&raw const self.vertex_stride),
                Some(&raw const self.vertex_offset),
            );
            context.IASetInputLayout(self.layout.as_ref());
            context.OMSetRenderTargets(Some(&self.targets), self.depth_view.as_ref());
        }
    }
}

/// The back buffer, its description, and the render target view on it.
struct Target {
    texture: ID3D11Texture2D,
    desc: D3D11_TEXTURE2D_DESC,
    view: ID3D11RenderTargetView,
}

pub struct Painter {
    device: ID3D11Device,
    immediate: ID3D11DeviceContext,
    vertices: Dynamic,
    indices: Dynamic,
    constants: Dynamic,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    sampler: ID3D11SamplerState,
    textures: HashMap<TextureId, Texture>,
    /// The game's back buffer and the view Angelica draws into it with,
    /// made once for that buffer; dropped before the game resizes it.
    target: Option<Target>,
}

fn needed<T>(value: Option<T>, label: &str) -> Result<T, String> {
    value.ok_or_else(|| format!("D3D11 returned no {label}"))
}
fn shader(entry: windows::core::PCSTR, target: windows::core::PCSTR) -> Result<Vec<u8>, String> {
    let mut code: Option<ID3DBlob> = None;
    let mut error: Option<ID3DBlob> = None;
    // SAFETY: source, entry and target live for the call; no include handler or external code.
    unsafe {
        D3DCompile(
            SHADER.as_ptr().cast(),
            SHADER.len(),
            s!("angelica"),
            None,
            None,
            entry,
            target,
            0,
            0,
            &raw mut code,
            Some(&raw mut error),
        )
    }
    .map_err(|e| e.to_string())?;
    let blob = needed(code, "shader bytecode")?;
    // SAFETY: ID3DBlob owns readable bytecode for exactly GetBufferSize bytes.
    Ok(unsafe {
        std::slice::from_raw_parts(blob.GetBufferPointer().cast::<u8>(), blob.GetBufferSize())
    }
    .to_vec())
}

impl Painter {
    /// Why the game's device stopped working, once it has.
    pub fn removed(&self) -> Option<String> {
        // SAFETY: a query of the owned device with no other precondition.
        unsafe { self.device.GetDeviceRemovedReason() }
            .err()
            .map(|e| format!("{:#010X} {}", e.code().0, e.message()))
    }

    /// The graphics adapter, its driver, the device, and `chain`, in one
    /// line for the log.
    pub fn describe(&self, chain: &IDXGISwapChain) -> String {
        let mut parts = Vec::new();
        // SAFETY: queries of owned COM objects into owned outputs.
        unsafe {
            if let Ok(adapter) = self
                .device
                .cast::<IDXGIDevice>()
                .and_then(|device| device.GetAdapter())
            {
                if let Ok(desc) = adapter.GetDesc() {
                    let name = String::from_utf16_lossy(&desc.Description);
                    parts.push(format!(
                        "{} (vendor {:04x}, device {:04x})",
                        name.trim_end_matches('\0'),
                        desc.VendorId,
                        desc.DeviceId
                    ));
                }
                if let Ok(version) = adapter.CheckInterfaceSupport(&IDXGIDevice::IID) {
                    let v = version.cast_unsigned();
                    parts.push(format!(
                        "driver {}.{}.{}.{}",
                        v >> 48,
                        (v >> 32) & 0xffff,
                        (v >> 16) & 0xffff,
                        v & 0xffff
                    ));
                }
            }
            parts.push(format!(
                "feature level {:#x}, device flags {:#x}",
                self.device.GetFeatureLevel().0,
                self.device.GetCreationFlags()
            ));
            if let Ok(desc) = chain.GetDesc() {
                parts.push(format!(
                    "swap chain {}x{}, format {}, {} buffers, effect {}, flags {:#x}, windowed {}",
                    desc.BufferDesc.Width,
                    desc.BufferDesc.Height,
                    desc.BufferDesc.Format.0,
                    desc.BufferCount,
                    desc.SwapEffect.0,
                    desc.Flags,
                    desc.Windowed.as_bool()
                ));
            }
        }
        parts.join("; ")
    }

    pub fn new(chain: &IDXGISwapChain) -> Result<Self, String> {
        // SAFETY: swap chain is a Windows-owned COM object; all output options are valid.
        let device: ID3D11Device = unsafe { chain.GetDevice() }.map_err(|e| e.to_string())?;
        // SAFETY: obtaining the device's owned immediate context has no other precondition.
        let immediate = unsafe { device.GetImmediateContext() }.map_err(|e| e.to_string())?;
        let vs = shader(s!("vertex"), s!("vs_4_0"))?;
        let ps = shader(s!("pixel"), s!("ps_4_0"))?;
        let (mut vertex, mut pixel, mut layout, mut blend, mut raster, mut sampler) =
            (None, None, None, None, None, None);
        let input = [
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("POSITION"),
                Format: DXGI_FORMAT_R32G32_FLOAT,
                AlignedByteOffset: 0,
                ..Default::default()
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("TEXCOORD"),
                Format: DXGI_FORMAT_R32G32_FLOAT,
                AlignedByteOffset: 8,
                ..Default::default()
            },
            D3D11_INPUT_ELEMENT_DESC {
                SemanticName: s!("COLOR"),
                Format: DXGI_FORMAT_R8G8B8A8_UNORM,
                AlignedByteOffset: 16,
                ..Default::default()
            },
        ];
        let mut blend_desc = D3D11_BLEND_DESC::default();
        blend_desc.RenderTarget[0] = D3D11_RENDER_TARGET_BLEND_DESC {
            BlendEnable: true.into(),
            SrcBlend: D3D11_BLEND_ONE,
            DestBlend: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOp: D3D11_BLEND_OP_ADD,
            SrcBlendAlpha: D3D11_BLEND_ONE,
            DestBlendAlpha: D3D11_BLEND_INV_SRC_ALPHA,
            BlendOpAlpha: D3D11_BLEND_OP_ADD,
            RenderTargetWriteMask: u8::try_from(D3D11_COLOR_WRITE_ENABLE_ALL.0)
                .map_err(|e| e.to_string())?,
        };
        let raster_desc = D3D11_RASTERIZER_DESC {
            FillMode: D3D11_FILL_SOLID,
            CullMode: D3D11_CULL_NONE,
            ScissorEnable: true.into(),
            DepthClipEnable: true.into(),
            ..Default::default()
        };
        let sampler_desc = D3D11_SAMPLER_DESC {
            Filter: D3D11_FILTER_MIN_MAG_MIP_LINEAR,
            AddressU: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressV: D3D11_TEXTURE_ADDRESS_CLAMP,
            AddressW: D3D11_TEXTURE_ADDRESS_CLAMP,
            ComparisonFunc: D3D11_COMPARISON_ALWAYS,
            MaxLOD: f32::MAX,
            ..Default::default()
        };
        // SAFETY: immutable descriptors and shader byte buffers match the declared layouts;
        // device calls copy their inputs and initialize the provided output options.
        unsafe {
            device
                .CreateVertexShader(&vs, None, Some(&raw mut vertex))
                .map_err(|e| e.to_string())?;
            device
                .CreatePixelShader(&ps, None, Some(&raw mut pixel))
                .map_err(|e| e.to_string())?;
            device
                .CreateInputLayout(&input, &vs, Some(&raw mut layout))
                .map_err(|e| e.to_string())?;
            device
                .CreateBlendState(&raw const blend_desc, Some(&raw mut blend))
                .map_err(|e| e.to_string())?;
            device
                .CreateRasterizerState(&raw const raster_desc, Some(&raw mut raster))
                .map_err(|e| e.to_string())?;
            device
                .CreateSamplerState(&raw const sampler_desc, Some(&raw mut sampler))
                .map_err(|e| e.to_string())?;
        }
        Ok(Self {
            device,
            immediate,
            vertices: Dynamic::new(D3D11_BIND_VERTEX_BUFFER),
            indices: Dynamic::new(D3D11_BIND_INDEX_BUFFER),
            constants: Dynamic::new(D3D11_BIND_CONSTANT_BUFFER),
            vertex: needed(vertex, "vertex shader")?,
            pixel: needed(pixel, "pixel shader")?,
            layout: needed(layout, "input layout")?,
            blend: needed(blend, "blend state")?,
            raster: needed(raster, "raster state")?,
            sampler: needed(sampler, "sampler")?,
            textures: HashMap::new(),
            target: None,
        })
    }

    fn update_texture(
        &mut self,
        id: TextureId,
        delta: &egui::epaint::ImageDelta,
    ) -> Result<(), String> {
        let egui::ImageData::Color(image) = &delta.image;
        if image.size[0] == 0
            || image.size[1] == 0
            || image.size[0] > 16384
            || image.size[1] > 16384
        {
            return Err("invalid egui texture dimensions".into());
        }
        let (size, mut bytes) = if let Some(pos) = delta.pos {
            let old = self
                .textures
                .get(&id)
                .ok_or("partial update of an unknown texture")?;
            if pos[0] + image.size[0] > old.size[0] || pos[1] + image.size[1] > old.size[1] {
                return Err("texture update lies outside the image".into());
            }
            (old.size, old.bytes.clone())
        } else {
            (image.size, vec![0; image.size[0] * image.size[1] * 4])
        };
        let pos = delta.pos.unwrap_or([0, 0]);
        for y in 0..image.size[1] {
            for x in 0..image.size[0] {
                let at = ((pos[1] + y) * size[0] + pos[0] + x) * 4;
                bytes[at..at + 4].copy_from_slice(&image.pixels[y * image.size[0] + x].to_array());
            }
        }
        let desc = D3D11_TEXTURE2D_DESC {
            Width: u32::try_from(size[0]).map_err(|e| e.to_string())?,
            Height: u32::try_from(size[1]).map_err(|e| e.to_string())?,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_R8G8B8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0.cast_unsigned(),
            ..Default::default()
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr().cast(),
            SysMemPitch: u32::try_from(size[0] * 4).map_err(|e| e.to_string())?,
            ..Default::default()
        };
        let mut texture = None;
        let mut view = None;
        // SAFETY: texture dimensions and row pitch match the owned RGBA buffer;
        // default view matches this shader-resource texture.
        unsafe {
            self.device
                .CreateTexture2D(
                    &raw const desc,
                    Some(&raw const data),
                    Some(&raw mut texture),
                )
                .map_err(|e| e.to_string())?;
            self.device
                .CreateShaderResourceView(&needed(texture, "texture")?, None, Some(&raw mut view))
                .map_err(|e| e.to_string())?;
        }
        self.textures.insert(
            id,
            Texture {
                size,
                bytes,
                view: needed(view, "texture view")?,
            },
        );
        Ok(())
    }

    pub fn paint(
        &mut self,
        chain: &IDXGISwapChain,
        primitives: &[ClippedPrimitive],
        delta: &mut egui::TexturesDelta,
        scale: f32,
    ) -> Result<(), String> {
        // Drain even on failure so egui's unapplied-delta drop assertion cannot panic later.
        let updates = std::mem::take(&mut delta.set);
        let freed = std::mem::take(&mut delta.free);
        let result = (|| {
            for (id, deltas) in updates {
                for update in deltas {
                    self.update_texture(id, &update)?;
                }
            }
            self.draw(chain, primitives, scale)
        })();
        for id in freed {
            self.textures.remove(&id);
        }
        result
    }

    /// Drops the view on the game's back buffer, so the game can resize it;
    /// in the game, the resize hook drops the whole painter instead.
    #[cfg(test)]
    fn release_target(&mut self) {
        self.target = None;
    }

    fn draw(
        &mut self,
        chain: &IDXGISwapChain,
        primitives: &[ClippedPrimitive],
        scale: f32,
    ) -> Result<(), String> {
        let meshes = meshes(primitives, &self.textures)?;
        // Nothing to draw: the game's back buffer and context stay untouched.
        if meshes.is_empty() {
            return Ok(());
        }
        let (target_desc, target_view) = target(&self.device, &mut self.target, chain)?;
        let width = f32::from(u16::try_from(target_desc.Width).map_err(|e| e.to_string())?);
        let height = f32::from(u16::try_from(target_desc.Height).map_err(|e| e.to_string())?);
        let linear = matches!(
            target_desc.Format,
            DXGI_FORMAT_R8G8B8A8_UNORM_SRGB
                | DXGI_FORMAT_B8G8R8A8_UNORM_SRGB
                | DXGI_FORMAT_R16G16B16A16_FLOAT
        );
        let screen: Vec<u8> = [width, height, f32::from(u8::from(linear)), 0.]
            .into_iter()
            .flat_map(f32::to_le_bytes)
            .collect();
        let (vertices, indices) = pack(&meshes, scale);
        let constant = self.constants.with_room(&self.device, screen.len())?;
        write(&self.immediate, &constant, &screen)?;
        let vertex_buffer = if vertices.is_empty() {
            None
        } else {
            let buffer = self.vertices.with_room(&self.device, vertices.len())?;
            write(&self.immediate, &buffer, &vertices)?;
            Some(buffer)
        };
        let index_buffer = if indices.is_empty() {
            None
        } else {
            let buffer = self.indices.with_room(&self.device, indices.len())?;
            write(&self.immediate, &buffer, &indices)?;
            Some(buffer)
        };
        let viewport = D3D11_VIEWPORT {
            Width: width,
            Height: height,
            MinDepth: 0.,
            MaxDepth: 1.,
            ..Default::default()
        };
        // SAFETY: Present runs on the game's rendering thread, which owns the
        // immediate context; every object set is owned and alive, and the
        // game's state is read first and put back below.
        let saved = unsafe { Saved::read(&self.immediate) };
        let result = (|| {
            // SAFETY: as above; the vertex stride matches the input layout.
            unsafe {
                let context = &self.immediate;
                context.OMSetRenderTargets(Some(&[Some(target_view)]), None);
                context.OMSetBlendState(&self.blend, None, u32::MAX);
                context.OMSetDepthStencilState(None, 0);
                context.RSSetState(&self.raster);
                context.RSSetViewports(Some(&[viewport]));
                context.IASetInputLayout(&self.layout);
                context.IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
                context.IASetVertexBuffers(
                    0,
                    1,
                    Some(&raw const vertex_buffer),
                    Some(&20),
                    Some(&0),
                );
                context.IASetIndexBuffer(index_buffer.as_ref(), DXGI_FORMAT_R32_UINT, 0);
                context.VSSetShader(&self.vertex, None);
                context.VSSetConstantBuffers(0, Some(&[Some(constant.clone())]));
                context.PSSetConstantBuffers(0, Some(&[Some(constant)]));
                context.PSSetShader(&self.pixel, None);
                context.PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
                context.GSSetShader(None, None);
                context.HSSetShader(None, None);
                context.DSSetShader(None, None);
            }
            let (mut first_index, mut first_vertex) = (0u32, 0i32);
            for (mesh, view, clip) in &meshes {
                let count = u32::try_from(mesh.indices.len()).map_err(|e| e.to_string())?;
                if let Some(scissor) = scissor(*clip, scale, width, height)? {
                    // SAFETY: as above; the indices of this mesh were validated
                    // against its vertices, which start at `first_vertex`.
                    unsafe {
                        self.immediate
                            .PSSetShaderResources(0, Some(&[Some((*view).clone())]));
                        self.immediate.RSSetScissorRects(Some(&[scissor]));
                        self.immediate.DrawIndexed(count, first_index, first_vertex);
                    }
                }
                first_index += count;
                first_vertex += i32::try_from(mesh.vertices.len()).map_err(|e| e.to_string())?;
            }
            Ok(())
        })();
        // SAFETY: restores what was read above on the same context and thread.
        unsafe { saved.restore(&self.immediate) };
        result
    }
}

/// The vertices of every mesh in order, in pixels, and their indices, each
/// relative to its mesh.
fn pack(
    meshes: &[(&egui::epaint::Mesh, &ID3D11ShaderResourceView, egui::Rect)],
    scale: f32,
) -> (Vec<u8>, Vec<u8>) {
    let mut vertices = Vec::new();
    let mut indices = Vec::new();
    for (mesh, _, _) in meshes {
        for vertex in &mesh.vertices {
            for v in [
                vertex.pos.x * scale,
                vertex.pos.y * scale,
                vertex.uv.x,
                vertex.uv.y,
            ] {
                vertices.extend_from_slice(&v.to_le_bytes());
            }
            vertices.extend_from_slice(&vertex.color.to_array());
        }
        indices.extend(mesh.indices.iter().flat_map(|i| i.to_le_bytes()));
    }
    (vertices, indices)
}

/// The back buffer's description and the view on it, made again only when
/// the swap chain hands out another buffer.
fn target(
    device: &ID3D11Device,
    cached: &mut Option<Target>,
    chain: &IDXGISwapChain,
) -> Result<(D3D11_TEXTURE2D_DESC, ID3D11RenderTargetView), String> {
    // SAFETY: index zero is the swap chain's current back buffer; COM type is queried.
    let texture: ID3D11Texture2D = unsafe { chain.GetBuffer(0) }.map_err(|e| e.to_string())?;
    if let Some(target) = cached.as_ref()
        && target.texture.as_raw() == texture.as_raw()
    {
        return Ok((target.desc, target.view.clone()));
    }
    *cached = None;
    let mut desc = D3D11_TEXTURE2D_DESC::default();
    let mut view = None;
    // SAFETY: owned D3D texture and valid output descriptors; default render target view.
    unsafe {
        texture.GetDesc(&raw mut desc);
        device
            .CreateRenderTargetView(&texture, None, Some(&raw mut view))
            .map_err(|e| e.to_string())?;
    }
    let view = needed(view, "render target view")?;
    *cached = Some(Target {
        texture,
        desc,
        view: view.clone(),
    });
    Ok((desc, view))
}

/// The meshes to draw, each with its texture and clip rectangle.
fn meshes<'a>(
    primitives: &'a [ClippedPrimitive],
    textures: &'a HashMap<TextureId, Texture>,
) -> Result<
    Vec<(
        &'a egui::epaint::Mesh,
        &'a ID3D11ShaderResourceView,
        egui::Rect,
    )>,
    String,
> {
    let mut meshes = Vec::new();
    for primitive in primitives {
        let Primitive::Mesh(mesh) = &primitive.primitive else {
            return Err("custom paint callbacks are not supported".into());
        };
        if mesh.indices.is_empty() {
            continue;
        }
        if !mesh.is_valid() {
            return Err("egui mesh contains invalid indices".into());
        }
        let texture = textures
            .get(&mesh.texture_id)
            .ok_or("mesh references an unknown texture")?;
        meshes.push((mesh, &texture.view, primitive.clip_rect));
    }
    Ok(meshes)
}

/// The scissor rectangle in pixels for a clip rectangle in points; none
/// when nothing of it is on the target.
fn scissor(clip: egui::Rect, scale: f32, width: f32, height: f32) -> Result<Option<RECT>, String> {
    let rect = RECT {
        left: (clip.min.x * scale)
            .floor()
            .clamp(0., width)
            .to_i32()
            .ok_or("invalid clip left")?,
        top: (clip.min.y * scale)
            .floor()
            .clamp(0., height)
            .to_i32()
            .ok_or("invalid clip top")?,
        right: (clip.max.x * scale)
            .ceil()
            .clamp(0., width)
            .to_i32()
            .ok_or("invalid clip right")?,
        bottom: (clip.max.y * scale)
            .ceil()
            .clamp(0., height)
            .to_i32()
            .ok_or("invalid clip bottom")?,
    };
    Ok((rect.right > rect.left && rect.bottom > rect.top).then_some(rect))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::{
        Direct3D::{D3D_DRIVER_TYPE_WARP, D3D_PRIMITIVE_TOPOLOGY_POINTLIST},
        Direct3D11::{
            D3D11_CPU_ACCESS_READ, D3D11_MAP_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_USAGE_STAGING,
        },
    };

    fn center_pixel(chain: &IDXGISwapChain, painter: &Painter) -> [u8; 4] {
        // SAFETY: test owns this live swap chain and queries its documented texture buffer.
        let target: ID3D11Texture2D = unsafe { chain.GetBuffer(0) }.unwrap();
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        // SAFETY: writable texture description has the required size.
        unsafe {
            target.GetDesc(&raw mut desc);
        }
        desc.Usage = D3D11_USAGE_STAGING;
        desc.BindFlags = 0;
        desc.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0.cast_unsigned();
        let mut staging = None;
        // SAFETY: matching texture dimensions and an output option owned by this test.
        unsafe {
            painter
                .device
                .CreateTexture2D(&raw const desc, None, Some(&raw mut staging))
        }
        .unwrap();
        let staging = staging.unwrap();
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        // SAFETY: staging has read access and matches the source; Map synchronizes the copy.
        unsafe {
            painter.immediate.CopyResource(&staging, &target);
            painter
                .immediate
                .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&raw mut mapped))
                .unwrap();
        }
        let offset =
            usize::try_from((desc.Height / 2) * mapped.RowPitch + (desc.Width / 2) * 4).unwrap();
        // SAFETY: midpoint lies within the mapped texture and four bytes are one RGBA pixel.
        let pixel = unsafe {
            mapped
                .pData
                .cast::<u8>()
                .add(offset)
                .cast::<[u8; 4]>()
                .read_unaligned()
        };
        // SAFETY: the staging resource is still mapped by this test.
        unsafe {
            painter.immediate.Unmap(&staging, 0);
        }
        pixel
    }

    #[test]
    fn warp_draws_pixels_restores_game_state_and_releases_resize_references() {
        let (window, chain) = super::super::overlay::create_chain(D3D_DRIVER_TYPE_WARP).unwrap();
        let mut painter = Painter::new(&chain).unwrap();
        let described = painter.describe(&chain);
        assert!(
            described.contains("feature level") && described.contains("swap chain"),
            "{described}"
        );
        assert_eq!(painter.removed(), None);
        let original = D3D11_VIEWPORT {
            Width: 17.,
            Height: 19.,
            MaxDepth: 1.,
            ..Default::default()
        };
        let game_scissor = RECT {
            left: 1,
            top: 2,
            right: 3,
            bottom: 4,
        };
        // SAFETY: owned immediate context accepts a single valid viewport descriptor.
        unsafe {
            painter.immediate.RSSetViewports(Some(&[original]));
            painter.immediate.RSSetScissorRects(Some(&[game_scissor]));
            painter
                .immediate
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
        }
        let mut mesh = egui::epaint::Mesh::default();
        for pos in [
            egui::pos2(0., 0.),
            egui::pos2(32., 0.),
            egui::pos2(32., 32.),
            egui::pos2(0., 32.),
        ] {
            mesh.vertices.push(egui::epaint::Vertex {
                pos,
                uv: egui::pos2(0.5, 0.5),
                color: egui::Color32::RED,
            });
        }
        mesh.indices = vec![0, 1, 2, 0, 2, 3];
        let primitives = [ClippedPrimitive {
            clip_rect: egui::Rect::EVERYTHING,
            primitive: Primitive::Mesh(mesh),
        }];
        let mut textures = egui::TexturesDelta::default();
        textures
            .set
            .entry(TextureId::default())
            .or_default()
            .push(egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], egui::Color32::WHITE),
                egui::TextureOptions::LINEAR,
            ));
        painter
            .paint(&chain, &primitives, &mut textures, 1.)
            .unwrap();
        assert_eq!(center_pixel(&chain, &painter), [255, 0, 0, 255]);
        let mut restored = D3D11_VIEWPORT::default();
        let mut count = 1;
        // SAFETY: one-element output storage matches the supplied viewport count.
        unsafe {
            painter
                .immediate
                .RSGetViewports(&raw mut count, Some(&raw mut restored));
        }
        assert_eq!(count, 1);
        let mut scissor = RECT::default();
        let mut scissors = 1;
        // SAFETY: one-element output storage matches the supplied rectangle count.
        let topology = unsafe {
            painter
                .immediate
                .RSGetScissorRects(&raw mut scissors, Some(&raw mut scissor));
            painter.immediate.IAGetPrimitiveTopology()
        };
        assert_eq!((scissors, scissor), (1, game_scissor));
        assert_eq!(topology, D3D_PRIMITIVE_TOPOLOGY_POINTLIST);
        assert_eq!(restored.Width.to_bits(), original.Width.to_bits());
        assert_eq!(restored.Height.to_bits(), original.Height.to_bits());
        painter.release_target();
        // SAFETY: the view on the back buffer is dropped and center_pixel keeps
        // no reference; private window.
        unsafe {
            chain.ResizeBuffers(
                1,
                64,
                64,
                DXGI_FORMAT_R8G8B8A8_UNORM,
                windows::Win32::Graphics::Dxgi::DXGI_SWAP_CHAIN_FLAG(0),
            )
        }
        .unwrap();
        painter
            .paint(&chain, &primitives, &mut textures, 2.)
            .unwrap();
        assert_eq!(center_pixel(&chain, &painter), [255, 0, 0, 255]);
        drop(painter);
        drop(chain);
        // SAFETY: no resources use this private test window anymore.
        unsafe {
            windows_sys::Win32::UI::WindowsAndMessaging::DestroyWindow(window as _);
        }
    }
}
