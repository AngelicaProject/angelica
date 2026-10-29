use super::*;

const SHADER: &str = r"
struct Uniforms { scale: vec2<f32>, tint: vec4<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(0) @binding(1) var image: texture_2d<f32>;
@group(0) @binding(2) var smp: sampler;
struct Out { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>) -> Out {
    var o: Out;
    o.pos = vec4<f32>(pos * u.scale, 0.0, 1.0);
    o.uv = uv;
    return o;
}
@fragment fn fs(i: Out) -> @location(0) vec4<f32> {
    return textureSample(image, smp, i.uv) * u.tint;
}
";

/// Stands in for the GPU's compiler.
#[allow(clippy::unnecessary_wraps)]
fn compiled(_: &str, entry: &str, _: bool) -> std::result::Result<Vec<u8>, String> {
    Ok(entry.as_bytes().to_vec())
}

fn area(width: u32, height: u32) -> Rect {
    Rect {
        x: 0,
        y: 0,
        width,
        height,
    }
}

fn kind<T: std::fmt::Debug>(result: Result<T>) -> Kind {
    result.unwrap_err().kind
}

const VERTEX: Usage = Usage {
    vertex: true,
    index: false,
    uniform: false,
};

const UNIFORM: Usage = Usage {
    vertex: false,
    index: false,
    uniform: true,
};

/// A pipeline of [`SHADER`] for two float pairs per vertex.
fn pipeline(gpu: &Gpu, plugin: &str) -> u32 {
    let shader = gpu.create_shader(plugin, SHADER).unwrap();
    gpu.create_pipeline(
        plugin,
        shader,
        "vs",
        "fs",
        16,
        vec![
            Attribute {
                location: 0,
                format: VertexFormat::Float32x2,
                offset: 0,
            },
            Attribute {
                location: 1,
                format: VertexFormat::Float32x2,
                offset: 8,
            },
        ],
        Topology::TriangleList,
        Blend::Alpha,
        &compiled,
    )
    .unwrap()
}

#[test]
fn wgsl_becomes_hlsl_with_registers_by_binding_and_its_entry_points() {
    let (hlsl, entries) = translate(SHADER).unwrap();
    assert!(hlsl.contains("register(b0)"), "{hlsl}");
    assert!(hlsl.contains("register(t1)"), "{hlsl}");
    assert!(hlsl.contains("register(s2)"), "{hlsl}");
    assert!(hlsl.contains(": LOC1"), "{hlsl}");
    assert_eq!(
        entries
            .iter()
            .map(|entry| (entry.vertex, entry.wgsl.as_str(), entry.locations.clone()))
            .collect::<Vec<_>>(),
        [(true, "vs", vec![0, 1]), (false, "fs", Vec::new())]
    );
}

#[test]
fn a_shader_outside_group_0_or_invalid_is_refused_with_its_error() {
    let other_group = SHADER.replace("@group(0) @binding(2)", "@group(1) @binding(2)");
    assert!(
        translate(&other_group)
            .unwrap_err()
            .message
            .contains("group 0")
    );
    let far = SHADER.replace("@binding(2)", "@binding(9)");
    assert!(
        translate(&far)
            .unwrap_err()
            .message
            .contains("bindings 0 to 7")
    );
    let broken = translate("@vertex fn vs() -> vec4<f32> { return 1; }").unwrap_err();
    assert_eq!(broken.kind, Kind::InvalidArgument);
    assert!(broken.message.contains("not valid") || broken.message.contains("parse"));
}

#[test]
fn objects_belong_to_their_plugin() {
    let gpu = Gpu::default();
    let buffer = gpu.create_buffer("one", 64, VERTEX).unwrap();
    assert_eq!(
        kind(gpu.write_buffer("two", buffer, 0, vec![0; 4])),
        Kind::NotFound
    );
    gpu.write_buffer("one", buffer, 60, vec![0; 4]).unwrap();
    assert_eq!(
        kind(gpu.write_buffer("one", buffer, 61, vec![0; 4])),
        Kind::InvalidArgument,
        "past the end"
    );
}

#[test]
fn memory_and_object_counts_have_limits() {
    let gpu = Gpu::default();
    for _ in 0..4 {
        gpu.create_buffer("one", MOST_BUFFER, VERTEX).unwrap();
    }
    assert_eq!(
        kind(gpu.create_buffer("one", 16, VERTEX)),
        Kind::LimitExceeded
    );
    gpu.create_buffer("two", 16, VERTEX).unwrap();
    assert_eq!(
        kind(gpu.create_buffer("one", 17, UNIFORM)),
        Kind::InvalidArgument
    );
}

#[test]
fn a_render_resolves_its_objects_and_needs_a_pipeline_before_a_draw() {
    let gpu = Gpu::default();
    let surface = gpu.create_surface("one", area(64, 32)).unwrap();
    let pipeline = pipeline(&gpu, "one");
    let buffer = gpu.create_buffer("one", 64, VERTEX).unwrap();
    let draw = Local::Draw(Command::Draw {
        vertices: 3,
        instances: 1,
        first_vertex: 0,
        first_instance: 0,
    });
    assert_eq!(
        kind(gpu.render("one", surface, None, vec![draw.clone()])),
        Kind::InvalidArgument
    );
    assert_eq!(
        kind(gpu.render(
            "one",
            surface,
            None,
            vec![Local::IndexBuffer {
                buffer,
                offset: 0,
                wide: false
            }]
        )),
        Kind::InvalidArgument,
        "a vertex buffer is not an index buffer"
    );
    gpu.render(
        "one",
        surface,
        Some([0.0; 4]),
        vec![
            Local::Pipeline(pipeline),
            Local::VertexBuffer { buffer, offset: 0 },
            draw,
        ],
    )
    .unwrap();
    let frame = gpu.take();
    assert!(matches!(
        frame.ops[0],
        Op::Surface {
            width: 64,
            height: 32,
            ..
        }
    ));
    let Some(Op::Render { commands, .. }) = frame.ops.last() else {
        panic!("{:?}", frame.ops);
    };
    assert_eq!(commands.len(), 3);
    assert_eq!(frame.placed.len(), 1);
}

#[test]
fn a_clear_drops_the_renders_the_renderer_has_not_run() {
    let gpu = Gpu::default();
    let surface = gpu.create_surface("one", area(8, 8)).unwrap();
    gpu.render("one", surface, Some([1.0; 4]), Vec::new())
        .unwrap();
    gpu.render("one", surface, None, Vec::new()).unwrap();
    gpu.render("one", surface, Some([0.0; 4]), Vec::new())
        .unwrap();
    let renders = gpu
        .take()
        .ops
        .into_iter()
        .filter(|op| matches!(op, Op::Render { .. }))
        .count();
    assert_eq!(renders, 1);
}

#[test]
fn the_surface_clicked_last_is_on_top_and_a_stopped_plugin_leaves_nothing() {
    let gpu = Gpu::default();
    let first = gpu.create_surface("one", area(8, 8)).unwrap();
    gpu.create_surface("two", area(8, 8)).unwrap();
    let bottom = gpu.placed()[0].id;
    assert!(gpu.focus(Some(bottom)).is_none());
    assert_eq!(gpu.placed().last().unwrap().id, bottom);
    assert_eq!(gpu.focused("one"), Some(first));
    assert_eq!(gpu.focused("two"), None);
    gpu.remove_plugin("one");
    assert_eq!(gpu.placed().len(), 1);
    assert_eq!(gpu.focused("one"), None);
    assert!(gpu.take().ops.contains(&Op::Destroy(bottom)));
}

#[test]
fn a_pipeline_needs_its_entry_points_and_every_vertex_input() {
    let gpu = Gpu::default();
    let shader = gpu.create_shader("one", SHADER).unwrap();
    let attribute = Attribute {
        location: 0,
        format: VertexFormat::Float32x2,
        offset: 0,
    };
    let create = |vertex: &str, attributes: Vec<Attribute>, stride: u32| {
        gpu.create_pipeline(
            "one",
            shader,
            vertex,
            "fs",
            stride,
            attributes,
            Topology::TriangleList,
            Blend::Replace,
            &compiled,
        )
    };
    assert!(
        create("fs", vec![attribute], 8).is_err(),
        "fs is not a vertex entry"
    );
    assert!(
        create("vs", vec![attribute], 8)
            .unwrap_err()
            .message
            .contains("location 1")
    );
    assert!(create("vs", vec![attribute], 4).is_err(), "past the stride");
}
