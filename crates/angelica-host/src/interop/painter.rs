//! Egui triangles rendered on a private deferred D3D11 context.
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
            Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, Fxc::D3DCompile, ID3DBlob},
            Direct3D11::{
                D3D11_BIND_CONSTANT_BUFFER, D3D11_BIND_FLAG, D3D11_BIND_INDEX_BUFFER,
                D3D11_BIND_SHADER_RESOURCE, D3D11_BIND_VERTEX_BUFFER, D3D11_BLEND_DESC,
                D3D11_BLEND_INV_SRC_ALPHA, D3D11_BLEND_ONE, D3D11_BLEND_OP_ADD, D3D11_BUFFER_DESC,
                D3D11_COLOR_WRITE_ENABLE_ALL, D3D11_COMPARISON_ALWAYS, D3D11_CULL_NONE,
                D3D11_FILL_SOLID, D3D11_FILTER_MIN_MAG_MIP_LINEAR, D3D11_INPUT_ELEMENT_DESC,
                D3D11_RASTERIZER_DESC, D3D11_RENDER_TARGET_BLEND_DESC, D3D11_SAMPLER_DESC,
                D3D11_SUBRESOURCE_DATA, D3D11_TEXTURE_ADDRESS_CLAMP, D3D11_TEXTURE2D_DESC,
                D3D11_USAGE_IMMUTABLE, D3D11_VIEWPORT, ID3D11BlendState, ID3D11Buffer,
                ID3D11Device, ID3D11DeviceContext, ID3D11InputLayout, ID3D11PixelShader,
                ID3D11RasterizerState, ID3D11SamplerState, ID3D11ShaderResourceView,
                ID3D11Texture2D, ID3D11VertexShader,
            },
            Dxgi::{
                Common::{
                    DXGI_FORMAT_B8G8R8A8_UNORM_SRGB, DXGI_FORMAT_R8G8B8A8_UNORM,
                    DXGI_FORMAT_R8G8B8A8_UNORM_SRGB, DXGI_FORMAT_R16G16B16A16_FLOAT,
                    DXGI_FORMAT_R32_UINT, DXGI_FORMAT_R32G32_FLOAT, DXGI_SAMPLE_DESC,
                },
                IDXGISwapChain,
            },
        },
    },
    core::s,
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

pub struct Painter {
    device: ID3D11Device,
    immediate: ID3D11DeviceContext,
    deferred: ID3D11DeviceContext,
    vertex: ID3D11VertexShader,
    pixel: ID3D11PixelShader,
    layout: ID3D11InputLayout,
    blend: ID3D11BlendState,
    raster: ID3D11RasterizerState,
    sampler: ID3D11SamplerState,
    textures: HashMap<TextureId, Texture>,
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
    pub fn new(chain: &IDXGISwapChain) -> Result<Self, String> {
        // SAFETY: swap chain is a Windows-owned COM object; all output options are valid.
        let device: ID3D11Device = unsafe { chain.GetDevice() }.map_err(|e| e.to_string())?;
        // SAFETY: obtaining the device's owned immediate context has no other precondition.
        let immediate = unsafe { device.GetImmediateContext() }.map_err(|e| e.to_string())?;
        let mut deferred = None;
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
                .CreateDeferredContext(0, Some(&raw mut deferred))
                .map_err(|e| e.to_string())?;
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
            deferred: needed(deferred, "deferred context")?,
            vertex: needed(vertex, "vertex shader")?,
            pixel: needed(pixel, "pixel shader")?,
            layout: needed(layout, "input layout")?,
            blend: needed(blend, "blend state")?,
            raster: needed(raster, "raster state")?,
            sampler: needed(sampler, "sampler")?,
            textures: HashMap::new(),
        })
    }

    fn buffer(&self, bytes: &[u8], bind: D3D11_BIND_FLAG) -> Result<ID3D11Buffer, String> {
        let desc = D3D11_BUFFER_DESC {
            ByteWidth: u32::try_from(bytes.len()).map_err(|e| e.to_string())?,
            Usage: D3D11_USAGE_IMMUTABLE,
            BindFlags: bind.0.cast_unsigned(),
            ..Default::default()
        };
        let data = D3D11_SUBRESOURCE_DATA {
            pSysMem: bytes.as_ptr().cast(),
            ..Default::default()
        };
        let mut buffer = None;
        // SAFETY: byte buffer is alive and its length equals ByteWidth; D3D copies the data.
        unsafe {
            self.device.CreateBuffer(
                &raw const desc,
                Some(&raw const data),
                Some(&raw mut buffer),
            )
        }
        .map_err(|e| e.to_string())?;
        needed(buffer, "buffer")
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

    fn draw(
        &self,
        chain: &IDXGISwapChain,
        primitives: &[ClippedPrimitive],
        scale: f32,
    ) -> Result<(), String> {
        // SAFETY: index zero is the swap chain's current back buffer; COM type is queried.
        let target: ID3D11Texture2D = unsafe { chain.GetBuffer(0) }.map_err(|e| e.to_string())?;
        let mut target_desc = D3D11_TEXTURE2D_DESC::default();
        let mut target_view = None;
        // SAFETY: owned D3D texture and valid output descriptors; default render target view.
        unsafe {
            target.GetDesc(&raw mut target_desc);
            self.device
                .CreateRenderTargetView(&target, None, Some(&raw mut target_view))
                .map_err(|e| e.to_string())?;
        }
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
        let constant = self.buffer(&screen, D3D11_BIND_CONSTANT_BUFFER)?;
        let viewport = D3D11_VIEWPORT {
            Width: width,
            Height: height,
            MinDepth: 0.,
            MaxDepth: 1.,
            ..Default::default()
        };
        // SAFETY: this private deferred context records only our commands. Owned COM objects
        // are kept alive throughout recording, and ExecuteCommandList(true) restores game state.
        unsafe {
            self.deferred.ClearState();
            self.deferred.OMSetRenderTargets(Some(&[target_view]), None);
            self.deferred.OMSetBlendState(&self.blend, None, u32::MAX);
            self.deferred.RSSetState(&self.raster);
            self.deferred.RSSetViewports(Some(&[viewport]));
            self.deferred.IASetInputLayout(&self.layout);
            self.deferred
                .IASetPrimitiveTopology(D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
            self.deferred.VSSetShader(&self.vertex, None);
            self.deferred
                .VSSetConstantBuffers(0, Some(&[Some(constant.clone())]));
            self.deferred
                .PSSetConstantBuffers(0, Some(&[Some(constant)]));
            self.deferred.PSSetShader(&self.pixel, None);
            self.deferred
                .PSSetSamplers(0, Some(&[Some(self.sampler.clone())]));
        }
        for primitive in primitives {
            self.draw_mesh(primitive, scale, width, height)?;
        }
        let mut commands = None;
        // SAFETY: finished command list uses owned resources; restore preserves every game
        // immediate-context state. Finishing with false clears deferred back-buffer references.
        unsafe {
            self.deferred
                .FinishCommandList(false, Some(&raw mut commands))
                .map_err(|e| e.to_string())?;
            self.immediate
                .ExecuteCommandList(&needed(commands, "command list")?, true);
        }
        Ok(())
    }

    fn draw_mesh(
        &self,
        primitive: &ClippedPrimitive,
        scale: f32,
        width: f32,
        height: f32,
    ) -> Result<(), String> {
        let Primitive::Mesh(mesh) = &primitive.primitive else {
            return Err("custom paint callbacks are not supported".into());
        };
        if mesh.indices.is_empty() {
            return Ok(());
        }
        if !mesh.is_valid() {
            return Err("egui mesh contains invalid indices".into());
        }
        let texture = self
            .textures
            .get(&mesh.texture_id)
            .ok_or("mesh references an unknown texture")?;
        let mut vertices = Vec::with_capacity(mesh.vertices.len() * 20);
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
        let indices: Vec<u8> = mesh.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        let vertex_buffer = Some(self.buffer(&vertices, D3D11_BIND_VERTEX_BUFFER)?);
        let index_buffer = self.buffer(&indices, D3D11_BIND_INDEX_BUFFER)?;
        let clip = primitive.clip_rect;
        let scissor = RECT {
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
        if scissor.right <= scissor.left || scissor.bottom <= scissor.top {
            return Ok(());
        }
        // SAFETY: vertex stride matches POSITION/TEXCOORD/COLOR; indices were validated,
        // resource options remain alive while D3D copies and retains their references.
        unsafe {
            self.deferred.IASetVertexBuffers(
                0,
                1,
                Some(&raw const vertex_buffer),
                Some(&20),
                Some(&0),
            );
            self.deferred
                .IASetIndexBuffer(&index_buffer, DXGI_FORMAT_R32_UINT, 0);
            self.deferred
                .PSSetShaderResources(0, Some(&[Some(texture.view.clone())]));
            self.deferred.RSSetScissorRects(Some(&[scissor]));
            self.deferred.DrawIndexed(
                u32::try_from(mesh.indices.len()).map_err(|e| e.to_string())?,
                0,
                0,
            );
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Graphics::{
        Direct3D::D3D_DRIVER_TYPE_WARP,
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
        let original = D3D11_VIEWPORT {
            Width: 17.,
            Height: 19.,
            MaxDepth: 1.,
            ..Default::default()
        };
        // SAFETY: owned immediate context accepts a single valid viewport descriptor.
        unsafe {
            painter.immediate.RSSetViewports(Some(&[original]));
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
        assert_eq!(restored.Width.to_bits(), original.Width.to_bits());
        assert_eq!(restored.Height.to_bits(), original.Height.to_bits());
        // SAFETY: no back-buffer references survive paint or center_pixel; private window.
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
