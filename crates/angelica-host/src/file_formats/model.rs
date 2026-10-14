//! Checks of replacement models (`.mdl`): every count, offset, and range
//! the game follows lies inside the file, and every index a mesh, a shape,
//! or a bone table holds names something the model has. The layout is
//! the one the game reads from disk, versions 5 and 6.

/// The model versions the game ships.
const V5: u32 = 0x0100_0005;
const V6: u32 = 0x0100_0006;
/// The header before the stack.
const HEADER: usize = 0x44;
/// One vertex declaration: 17 elements of 8 bytes.
const DECLARATION: usize = 17 * 8;
/// The vertex element types and their sizes in bytes.
const ELEMENT_SIZES: [(u8, usize); 14] = [
    (0, 4),
    (1, 8),
    (2, 12),
    (3, 16),
    (5, 4),
    (6, 4),
    (7, 8),
    (8, 4),
    (9, 4),
    (10, 8),
    (13, 4),
    (14, 8),
    (16, 4),
    (17, 8),
];

/// Reads little-endian values in order and names what ended the file.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
    end: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, count: usize, what: &str) -> Result<&'a [u8], String> {
        let end = self
            .at
            .checked_add(count)
            .filter(|end| *end <= self.end)
            .ok_or_else(|| format!("a model ends before its {what}"))?;
        let taken = &self.bytes[self.at..end];
        self.at = end;
        Ok(taken)
    }

    fn u8(&mut self, what: &str) -> Result<u8, String> {
        Ok(self.take(1, what)?[0])
    }

    fn u32(&mut self, what: &str) -> Result<usize, String> {
        let bytes = self.take(4, what)?;
        usize::try_from(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
            .map_err(|_| format!("a model's {what} is too large"))
    }
}

fn word(bytes: &[u8], at: usize) -> usize {
    usize::try_from(u32::from_le_bytes([
        bytes[at],
        bytes[at + 1],
        bytes[at + 2],
        bytes[at + 3],
    ]))
    .unwrap_or(usize::MAX)
}

fn half(bytes: &[u8], at: usize) -> usize {
    usize::from(u16::from_le_bytes([bytes[at], bytes[at + 1]]))
}

/// A range of `count` items from `start` inside `total`.
fn inside(start: usize, count: usize, total: usize, what: &str) -> Result<(), String> {
    if start.checked_add(count).is_some_and(|end| end <= total) {
        Ok(())
    } else {
        Err(format!(
            "{what} {start} and {count} more pass the {total} there are"
        ))
    }
}

/// One vertex element: its stream, offset, and type.
struct Element {
    stream: usize,
    offset: usize,
    size: usize,
}

/// A mesh: what its vertices and indices take.
struct Mesh {
    vertices: usize,
    indices: usize,
    start: usize,
    material: usize,
    submesh_start: usize,
    submeshes: usize,
    bone_table: usize,
    offsets: [usize; 3],
    strides: [usize; 3],
    streams: usize,
}

/// A level of detail: its mesh ranges and its data in the file.
struct Lod {
    /// The meshes it draws: main, water, shadow, and vertical fog.
    ranges: [(usize, usize); 4],
    /// Its terrain shadow meshes.
    terrain: (usize, usize),
    vertex_size: usize,
    index_size: usize,
    vertex_offset: usize,
    index_offset: usize,
}

/// The counts of the model header.
struct Counts {
    meshes: usize,
    attributes: usize,
    submeshes: usize,
    materials: usize,
    bones: usize,
    bone_tables: usize,
    shapes: usize,
    shape_meshes: usize,
    shape_values: usize,
    lods: usize,
    element_ids: usize,
    terrain_meshes: usize,
    extra_lods: bool,
    terrain_submeshes: usize,
    bone_indices: usize,
}

/// A shape's shape meshes in each level: the first and the count.
type ShapeLevels = [(usize, usize); 3];

/// A shape mesh: the start of the mesh it changes in its level's indices,
/// and its first value and value count.
struct ShapeMesh {
    mesh_start: usize,
    first: usize,
    values: usize,
}

/// What the checks follow, read in the file's order.
struct Model<'a> {
    bytes: &'a [u8],
    /// Where the vertex and index data may start: after the runtime.
    data: usize,
    lods: usize,
    counts: Counts,
    declared: Vec<Vec<Element>>,
    levels: Vec<Lod>,
    meshes: Vec<Mesh>,
    /// Index offset, index count, bone start, and bone count of each.
    submeshes: Vec<(usize, usize, usize, usize)>,
    /// Shape meshes of each level, per shape.
    shapes: Vec<ShapeLevels>,
    shape_meshes: Vec<ShapeMesh>,
    shape_values: &'a [u8],
    bone_map: usize,
}

/// Checks a model; see the module's description.
///
/// # Errors
/// Names the first rule the model breaks.
pub(super) fn model(bytes: &[u8]) -> Result<(), String> {
    let model = read(bytes)?;
    check_levels(&model)?;
    check_meshes(&model)?;
    check_drawn(&model)?;
    check_shapes(&model)
}

/// The vertex declarations of the stack: each up to 16 elements of known
/// types and streams, ended by stream 255.
fn declarations(bytes: &[u8], count: usize) -> Result<Vec<Vec<Element>>, String> {
    let mut declared = Vec::with_capacity(count);
    for declaration in 0..count {
        let start = HEADER + declaration * DECLARATION;
        let mut elements = Vec::new();
        let mut ended = false;
        for element in 0..17 {
            let at = start + element * 8;
            let stream = bytes[at];
            if stream == 0xFF {
                ended = true;
                break;
            }
            let kind = bytes[at + 2];
            let size = ELEMENT_SIZES
                .iter()
                .find(|(known, _)| *known == kind)
                .map(|(_, size)| *size)
                .ok_or_else(|| format!("vertex element type {kind} is not one the game uses"))?;
            if stream >= 3 || bytes[at + 3] > 7 {
                return Err(
                    "a vertex element names a stream or a use the game does not have".into(),
                );
            }
            elements.push(Element {
                stream: usize::from(stream),
                offset: usize::from(bytes[at + 1]),
                size,
            });
        }
        if !ended {
            return Err("a vertex declaration does not end".into());
        }
        declared.push(elements);
    }
    Ok(declared)
}

/// Reads the file header, the stack, and the runtime.
fn read(bytes: &[u8]) -> Result<Model<'_>, String> {
    if bytes.len() < HEADER {
        return Err("a model is shorter than its header".into());
    }
    let version = u32::try_from(word(bytes, 0)).unwrap_or_default();
    if version != V5 && version != V6 {
        return Err(format!("model version {version:#x} is not 5 or 6"));
    }
    let (stack, runtime) = (word(bytes, 4), word(bytes, 8));
    let declaration_count = half(bytes, 12);
    let lods = usize::from(bytes[64]);
    if !(1..=3).contains(&lods) {
        return Err(format!("a model has 1 to 3 levels of detail, not {lods}"));
    }
    if stack != declaration_count * DECLARATION {
        return Err("the stack holds the vertex declarations exactly".into());
    }
    let data = HEADER
        .checked_add(stack)
        .and_then(|end| end.checked_add(runtime))
        .filter(|end| *end <= bytes.len())
        .ok_or("a model's stack and runtime pass its end")?;
    let declared = declarations(bytes, declaration_count)?;
    let mut r = Reader {
        bytes,
        at: HEADER + stack,
        end: data,
    };
    let strings = read_strings(&mut r)?;
    let string = |offset: usize| -> Result<(), String> {
        strings
            .get(offset..)
            .filter(|rest| rest.contains(&0))
            .map(drop)
            .ok_or_else(|| "a name does not lie inside the model's strings".into())
    };
    let counts = read_counts(&mut r)?;
    if counts.lods != lods {
        return Err("the model header's levels of detail differ from the file header's".into());
    }
    if counts.materials != half(bytes, 14) {
        return Err("the model header's materials differ from the file header's".into());
    }
    if declaration_count != counts.meshes {
        return Err(format!(
            "{declaration_count} vertex declarations for {} meshes",
            counts.meshes
        ));
    }
    for _ in 0..counts.element_ids {
        string(word(r.take(32, "element ids")?, 4))?;
    }
    let levels = read_levels(&mut r, &counts)?;
    let meshes = read_meshes(&mut r, &counts)?;
    for _ in 0..counts.attributes {
        string(r.u32("attributes")?)?;
    }
    r.take(20 * counts.terrain_meshes, "terrain shadow meshes")?;
    let mut submeshes = Vec::with_capacity(counts.submeshes);
    for _ in 0..counts.submeshes {
        let submesh = r.take(16, "submeshes")?;
        submeshes.push((
            word(submesh, 0),
            word(submesh, 4),
            half(submesh, 12),
            half(submesh, 14),
        ));
    }
    r.take(12 * counts.terrain_submeshes, "terrain shadow submeshes")?;
    for _ in 0..counts.materials + counts.bones {
        string(r.u32("material and bone names")?)?;
    }
    read_bone_tables(&mut r, version, &counts)?;
    let (shapes, shape_meshes) = read_shapes(&mut r, &counts, &string)?;
    let shape_values = r.take(4 * counts.shape_values, "shape values")?;
    let bone_map_size = r.u32("submesh bone map")?;
    r.take(bone_map_size, "submesh bone map")?;
    let padding = usize::from(r.u8("padding")?);
    r.take(padding, "padding")?;
    r.take(4 * 32 + counts.bones * 32, "bounding boxes")?;
    Ok(Model {
        bytes,
        data,
        lods,
        counts,
        declared,
        levels,
        meshes,
        submeshes,
        shapes,
        shape_meshes,
        shape_values,
        bone_map: bone_map_size / 2,
    })
}

/// The shapes, each with its shape meshes in every level, and the shape
/// meshes, each with its values.
fn read_shapes(
    r: &mut Reader<'_>,
    counts: &Counts,
    string: &dyn Fn(usize) -> Result<(), String>,
) -> Result<(Vec<ShapeLevels>, Vec<ShapeMesh>), String> {
    let mut shapes = Vec::with_capacity(counts.shapes);
    for _ in 0..counts.shapes {
        let shape = r.take(16, "shapes")?;
        string(word(shape, 0))?;
        let ranges: ShapeLevels =
            std::array::from_fn(|level| (half(shape, 4 + 2 * level), half(shape, 10 + 2 * level)));
        for &(start, count) in &ranges {
            inside(start, count, counts.shape_meshes, "shape meshes")?;
        }
        shapes.push(ranges);
    }
    let mut shape_meshes = Vec::with_capacity(counts.shape_meshes);
    for _ in 0..counts.shape_meshes {
        let shape_mesh = r.take(12, "shape meshes")?;
        let (values, first) = (word(shape_mesh, 4), word(shape_mesh, 8));
        inside(first, values, counts.shape_values, "shape values")?;
        shape_meshes.push(ShapeMesh {
            mesh_start: word(shape_mesh, 0),
            first,
            values,
        });
    }
    Ok((shapes, shape_meshes))
}

fn read_strings<'a>(r: &mut Reader<'a>) -> Result<&'a [u8], String> {
    r.take(4, "strings")?;
    let size = r.u32("strings")?;
    r.take(size, "strings")
}

fn read_counts(r: &mut Reader<'_>) -> Result<Counts, String> {
    let header = r.take(56, "model header")?;
    Ok(Counts {
        meshes: half(header, 4),
        attributes: half(header, 6),
        submeshes: half(header, 8),
        materials: half(header, 10),
        bones: half(header, 12),
        bone_tables: half(header, 14),
        shapes: half(header, 16),
        shape_meshes: half(header, 18),
        shape_values: half(header, 20),
        lods: usize::from(header[22]),
        element_ids: half(header, 24),
        terrain_meshes: usize::from(header[26]),
        extra_lods: header[27] & 0x10 != 0,
        terrain_submeshes: half(header, 38),
        bone_indices: half(header, 44),
    })
}

/// The three levels of detail, and the mesh ranges of the extra ones.
fn read_levels(r: &mut Reader<'_>, counts: &Counts) -> Result<Vec<Lod>, String> {
    let mut levels = Vec::with_capacity(3);
    for _ in 0..3 {
        let lod = r.take(60, "levels of detail")?;
        let range = |at: usize| (half(lod, at), half(lod, at + 2));
        levels.push(Lod {
            ranges: [range(0), range(12), range(16), range(24)],
            terrain: range(20),
            vertex_size: word(lod, 44),
            index_size: word(lod, 48),
            vertex_offset: word(lod, 52),
            index_offset: word(lod, 56),
        });
    }
    if counts.extra_lods {
        for _ in 0..3 {
            let extra = r.take(40, "extra levels of detail")?;
            for at in [0, 4, 8, 12] {
                inside(
                    half(extra, at),
                    half(extra, at + 2),
                    counts.meshes,
                    "meshes",
                )?;
            }
        }
    }
    Ok(levels)
}

fn read_meshes(r: &mut Reader<'_>, counts: &Counts) -> Result<Vec<Mesh>, String> {
    let mut meshes = Vec::with_capacity(counts.meshes);
    for _ in 0..counts.meshes {
        let mesh = r.take(36, "meshes")?;
        meshes.push(Mesh {
            vertices: half(mesh, 0),
            indices: word(mesh, 4),
            material: half(mesh, 8),
            submesh_start: half(mesh, 10),
            submeshes: half(mesh, 12),
            bone_table: half(mesh, 14),
            start: word(mesh, 16),
            offsets: [word(mesh, 20), word(mesh, 24), word(mesh, 28)],
            strides: [
                usize::from(mesh[32]),
                usize::from(mesh[33]),
                usize::from(mesh[34]),
            ],
            // The low 2 bits; the game keeps flags above them.
            streams: usize::from(mesh[35] & 3),
        });
    }
    Ok(meshes)
}

/// Reads the bone tables, each naming bones the model has: of version 5,
/// 64 bone slots and a count; of version 6, an offset and a count into the
/// bone indices that follow the tables.
fn read_bone_tables(r: &mut Reader<'_>, version: u32, counts: &Counts) -> Result<(), String> {
    let bytes = r.bytes;
    let mut tables: Vec<Vec<usize>> = Vec::with_capacity(counts.bone_tables);
    if version == V5 {
        for _ in 0..counts.bone_tables {
            let table = r.take(132, "bone tables")?;
            let count = word(table, 128);
            if count > 64 {
                return Err("a bone table holds at most 64 bones".into());
            }
            tables.push((0..count).map(|bone| half(table, 2 * bone)).collect());
        }
    } else {
        let entries = r.at;
        r.take(4 * counts.bone_tables, "bone tables")?;
        let indices = r.at;
        r.take(2 * counts.bone_indices, "bone tables")?;
        for table in 0..counts.bone_tables {
            let entry = entries + 4 * table;
            let (offset, count) = (half(bytes, entry), half(bytes, entry + 2));
            let start = entry + 4 * offset;
            if start < indices || start + 2 * count > indices + 2 * counts.bone_indices {
                return Err("a bone table lies outside the bone indices".into());
            }
            tables.push(
                (0..count)
                    .map(|bone| half(bytes, start + 2 * bone))
                    .collect(),
            );
        }
    }
    if tables.iter().flatten().any(|&bone| bone >= counts.bones) {
        return Err(format!(
            "a bone table names a bone past the {} there are",
            counts.bones
        ));
    }
    Ok(())
}

/// Every level's data lies in the file after the runtime, and its ranges
/// in their lists.
fn check_levels(model: &Model<'_>) -> Result<(), String> {
    for (level, lod) in model.levels.iter().enumerate().take(model.lods) {
        for (offset, size, what) in [
            (lod.vertex_offset, lod.vertex_size, "vertex"),
            (lod.index_offset, lod.index_size, "index"),
        ] {
            if size > 0
                && (offset < model.data
                    || offset
                        .checked_add(size)
                        .is_none_or(|end| end > model.bytes.len()))
            {
                return Err(format!("level {level}'s {what} data lies outside the file"));
            }
        }
        for &(start, count) in &lod.ranges {
            inside(start, count, model.counts.meshes, "meshes")?;
        }
        inside(
            lod.terrain.0,
            lod.terrain.1,
            model.counts.terrain_meshes,
            "terrain shadow meshes",
        )?;
    }
    Ok(())
}

/// Each mesh names a material, submeshes, and a bone table it has, and its
/// declaration fits its streams.
fn check_meshes(model: &Model<'_>) -> Result<(), String> {
    let counts = &model.counts;
    for (mesh, declared) in model.meshes.iter().zip(&model.declared) {
        if mesh.material >= counts.materials {
            return Err(format!(
                "a mesh names material {} of {}",
                mesh.material, counts.materials
            ));
        }
        inside(
            mesh.submesh_start,
            mesh.submeshes,
            counts.submeshes,
            "submeshes",
        )?;
        if mesh.bone_table >= counts.bone_tables && mesh.bone_table != 0xFF {
            return Err(format!("a mesh names bone table {}", mesh.bone_table));
        }
        for element in declared {
            if element.stream >= mesh.streams
                || element.offset + element.size > mesh.strides[element.stream]
            {
                return Err("a vertex element lies outside its stream's stride".into());
            }
        }
        let submeshes = &model.submeshes[mesh.submesh_start..mesh.submesh_start + mesh.submeshes];
        for &(offset, count, bone_start, bones) in submeshes {
            if offset < mesh.start || offset + count > mesh.start + mesh.indices {
                return Err("a submesh's indices lie outside its mesh's".into());
            }
            // A submesh without bones may name any start.
            if bones > 0 {
                inside(bone_start, bones, model.bone_map, "submesh bones")?;
            }
        }
    }
    Ok(())
}

/// Each mesh's vertices and indices lie in the data of every level that
/// draws it, and every index names one of its vertices.
fn check_drawn(model: &Model<'_>) -> Result<(), String> {
    for lod in model.levels.iter().take(model.lods) {
        let drawn = lod
            .ranges
            .iter()
            .flat_map(|&(start, count)| start..start + count);
        for index in drawn {
            let mesh = &model.meshes[index];
            for stream in 0..mesh.streams {
                let end = mesh.offsets[stream].saturating_add(mesh.vertices * mesh.strides[stream]);
                if end > lod.vertex_size {
                    return Err(format!(
                        "mesh {index}'s vertices lie outside its level's vertex data"
                    ));
                }
            }
            let end = mesh.start.saturating_add(mesh.indices);
            if end.saturating_mul(2) > lod.index_size {
                return Err(format!(
                    "mesh {index}'s indices lie outside its level's index data"
                ));
            }
            let indices =
                &model.bytes[lod.index_offset + 2 * mesh.start..lod.index_offset + 2 * end];
            if let Some(past) = indices
                .as_chunks::<2>()
                .0
                .iter()
                .map(|pair| usize::from(u16::from_le_bytes(*pair)))
                .find(|&vertex| vertex >= mesh.vertices)
            {
                return Err(format!(
                    "an index names vertex {past} of mesh {index}, which has {}",
                    mesh.vertices
                ));
            }
        }
    }
    Ok(())
}

/// A shape replaces indices of a mesh of a level with vertices of the same
/// mesh; the game writes them into the mesh's indices.
fn check_shapes(model: &Model<'_>) -> Result<(), String> {
    for (level, lod) in model.levels.iter().enumerate().take(model.lods) {
        let (first_mesh, mesh_count) = lod.ranges[0];
        let level_meshes = &model.meshes[first_mesh..first_mesh + mesh_count];
        for shape in &model.shapes {
            let (start, count) = shape[level];
            for shape_mesh in &model.shape_meshes[start..start + count] {
                let mesh = level_meshes
                    .iter()
                    .find(|mesh| mesh.start == shape_mesh.mesh_start)
                    .ok_or("a shape names no mesh of its level")?;
                for value in shape_mesh.first..shape_mesh.first + shape_mesh.values {
                    let base = half(model.shape_values, 4 * value);
                    let vertex = half(model.shape_values, 4 * value + 2);
                    if base >= mesh.indices || vertex >= mesh.vertices {
                        return Err(format!(
                            "a shape value changes index {base} to vertex {vertex} of a mesh of {} indices and {} vertices",
                            mesh.indices, mesh.vertices
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The parts of a test model that tests change.
    pub(crate) struct Parts {
        pub indices: [u16; 3],
        pub material: u16,
        pub bone: u16,
        pub element_offset: u8,
        pub stride: u8,
        pub vertex_size: u32,
    }

    impl Default for Parts {
        fn default() -> Self {
            Self {
                indices: [0, 1, 2],
                material: 0,
                bone: 0,
                element_offset: 0,
                stride: 12,
                vertex_size: 36,
            }
        }
    }

    /// A version 6 model of one triangle: one mesh, submesh, material,
    /// bone, and bone table, one level of detail.
    pub(crate) fn model_file(parts: &Parts) -> Vec<u8> {
        let put16 = |bytes: &mut Vec<u8>, value: u16| bytes.extend_from_slice(&value.to_le_bytes());
        let put32 = |bytes: &mut Vec<u8>, value: u32| bytes.extend_from_slice(&value.to_le_bytes());
        let mut stack = vec![0, parts.element_offset, 2, 0, 0, 0, 0, 0, 0xFF];
        stack.resize(DECLARATION, 0);
        let strings = b"mat.mtrl\0j_kosi\0atr\0".to_vec();
        let mut runtime = Vec::new();
        put16(&mut runtime, 3);
        put16(&mut runtime, 0);
        put32(&mut runtime, u32::try_from(strings.len()).unwrap());
        runtime.extend_from_slice(&strings);
        // The model header.
        let mut header = vec![0u8; 56];
        for (at, value) in [
            (4, 1u16),
            (6, 1),
            (8, 1),
            (10, 1),
            (12, 1),
            (14, 1),
            (44, 1),
        ] {
            header[at..at + 2].copy_from_slice(&value.to_le_bytes());
        }
        header[22] = 1;
        runtime.extend_from_slice(&header);
        let lods_at = runtime.len();
        runtime.resize(lods_at + 3 * 60, 0);
        runtime[lods_at + 2..lods_at + 4].copy_from_slice(&1u16.to_le_bytes());
        // The mesh.
        put16(&mut runtime, 3);
        put16(&mut runtime, 0);
        put32(&mut runtime, 3);
        for value in [parts.material, 0, 1, 0] {
            put16(&mut runtime, value);
        }
        for value in [0u32, 0, 0, 0] {
            put32(&mut runtime, value);
        }
        runtime.extend_from_slice(&[parts.stride, 0, 0, 1]);
        put32(&mut runtime, 16); // the attribute's name
        for value in [0u32, 3, 1] {
            put32(&mut runtime, value); // the submesh
        }
        put16(&mut runtime, 0);
        put16(&mut runtime, 1);
        put32(&mut runtime, 0); // the material's name
        put32(&mut runtime, 9); // the bone's name
        put16(&mut runtime, 1); // the bone table, its indices just after
        put16(&mut runtime, 1);
        put16(&mut runtime, parts.bone);
        put32(&mut runtime, 2); // the submesh bone map
        put16(&mut runtime, 0);
        runtime.push(0); // no padding
        runtime.resize(runtime.len() + 5 * 32, 0);
        let data = HEADER + stack.len() + runtime.len();
        let lod = lods_at + 44;
        for (at, value) in [
            (lod, parts.vertex_size),
            (lod + 4, 6),
            (lod + 8, u32::try_from(data).unwrap()),
            (lod + 12, u32::try_from(data + 36).unwrap()),
        ] {
            runtime[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        let mut bytes = Vec::new();
        put32(&mut bytes, V6);
        put32(&mut bytes, u32::try_from(stack.len()).unwrap());
        put32(&mut bytes, u32::try_from(runtime.len()).unwrap());
        put16(&mut bytes, 1);
        put16(&mut bytes, 1);
        for value in [data, 0, 0, data + 36, 0, 0, 36, 0, 0, 6, 0, 0] {
            put32(&mut bytes, u32::try_from(value).unwrap());
        }
        bytes.extend_from_slice(&[1, 0, 0, 0]);
        bytes.extend_from_slice(&stack);
        bytes.extend_from_slice(&runtime);
        for value in 0..9u32 {
            #[allow(clippy::cast_precision_loss)] // small coordinates
            let value = value as f32;
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for index in parts.indices {
            put16(&mut bytes, index);
        }
        bytes
    }

    #[test]
    fn models_name_only_what_they_have() {
        model(&model_file(&Parts::default())).unwrap();
        let refused = |parts: Parts, problem: &str| {
            let error = model(&model_file(&parts)).unwrap_err();
            assert!(error.contains(problem), "{error}");
        };
        refused(
            Parts {
                indices: [0, 1, 3],
                ..Parts::default()
            },
            "an index names vertex 3",
        );
        refused(
            Parts {
                material: 1,
                ..Parts::default()
            },
            "names material 1",
        );
        refused(
            Parts {
                bone: 1,
                ..Parts::default()
            },
            "bone past",
        );
        refused(
            Parts {
                element_offset: 4,
                ..Parts::default()
            },
            "outside its stream's stride",
        );
        refused(
            Parts {
                vertex_size: 30,
                ..Parts::default()
            },
            "outside its level's vertex data",
        );
        refused(
            Parts {
                vertex_size: 4096,
                ..Parts::default()
            },
            "outside the file",
        );
        let mut version = model_file(&Parts::default());
        version[0] = 7;
        assert!(model(&version).unwrap_err().contains("version"));
        let whole = model_file(&Parts::default());
        for cut in [10, HEADER + 20, whole.len() - 1] {
            assert!(model(&whole[..cut]).is_err(), "{cut}");
        }
    }

    #[test]
    fn damaged_models_are_refused_without_panicking() {
        let whole = model_file(&Parts::default());
        let mut mutator = crate::mutations::Mutator::new(11);
        for _ in 0..50_000 {
            let _ = model(&mutator.mutate(&whole));
        }
    }
}
