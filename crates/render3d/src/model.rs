//! Models from the art studio, read from the binary glTF files (`.glb`) its exporter writes (rts-engine,
//! `art/studio/export_gltf.py`), worked out with no GPU so they are tested on their own.
//!
//! Only what the exporter writes is read: triangle meshes with positions, normals and texture coordinates, a node
//! tree of translations, rotations and scales, and materials with one base colour texture (PNG). A material whose
//! extras say `"team_paint": true` is painted in the owner's colour, multiplied into its grey. Animations and skins
//! are skipped.
//!
//! A model comes out in view axes with its size still in metres: the file's `x` stays east, its `-z` (a model's
//! front) becomes north (`-y`), and its `y` (up) becomes `z`. The renderer scales each model to the unit it stands
//! for. Parts that turn on their own (a vehicle's `turret`, a defence's `head`) are kept apart, around their pivot.

use crate::json::{self, Value};
use crate::shapes::{Pose, Shape};
use view3d::maths::{Mat4, V3, mul};

/// One vertex: where it is, which way it faces, where it reads the texture, and 1 if it is team paint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vertex {
    pub pos: V3,
    pub normal: V3,
    pub uv: [f32; 2],
    pub team: f32,
}

/// The triangles of one material on one part.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    /// Whether this piece turns on its own about `pivot`: a turret or a defence's head.
    pub turns: bool,
    /// The point it turns about, in the model's space; zero for pieces that don't turn.
    pub pivot: V3,
    /// Vertices relative to `pivot`.
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Index into the model's images.
    pub image: usize,
}

/// An RGBA image, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub pieces: Vec<Piece>,
    pub images: Vec<Image>,
    /// The corners of the box round every vertex, in the model's space.
    pub min: V3,
    pub max: V3,
}

/// Names of the parts that turn on their own.
const TURNING: [&str; 2] = ["turret", "head"];

impl Model {
    /// How many triangles it has.
    pub fn triangles(&self) -> usize {
        self.pieces.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// Reads a `.glb` file.
    pub fn from_glb(bytes: &[u8]) -> Result<Model, String> {
        let word = |at: usize| -> Result<u32, String> {
            let b = bytes.get(at..at + 4).ok_or("glb: cut short")?;
            Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        };
        if bytes.get(..4) != Some(b"glTF") || word(4)? != 2 {
            return Err("not a glTF 2 binary".into());
        }
        // Chunks: the JSON first, then the binary buffer.
        let (mut header, mut bin) = (None, &[][..]);
        let mut at = 12;
        while at + 8 <= bytes.len() {
            let (len, kind) = (word(at)? as usize, word(at + 4)?);
            let chunk = bytes.get(at + 8..at + 8 + len).ok_or("glb: a chunk runs past the end")?;
            match kind {
                0x4E4F_534A => header = Some(std::str::from_utf8(chunk).map_err(|_| "glb: JSON is not UTF-8")?),
                0x004E_4942 => bin = chunk,
                _ => {}
            }
            at += 8 + len;
        }
        let doc = json::parse(header.ok_or("glb: no JSON chunk")?)?;
        Reader { doc: &doc, bin }.model()
    }
}

struct Reader<'a> {
    doc: &'a Value,
    bin: &'a [u8],
}

/// A column-major 4 by 4 matrix, as glTF stores them.
type M4 = [f32; 16];

const IDENTITY: M4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0];

fn mul4(a: &M4, b: &M4) -> M4 {
    let mut out = [0.0; 16];
    for c in 0..4 {
        for r in 0..4 {
            out[c * 4 + r] = (0..4).map(|k| a[k * 4 + r] * b[c * 4 + k]).sum();
        }
    }
    out
}

fn point(m: &M4, p: V3) -> V3 {
    [0, 1, 2].map(|r| m[r] * p[0] + m[4 + r] * p[1] + m[8 + r] * p[2] + m[12 + r])
}

fn direction(m: &M4, d: V3) -> V3 {
    view3d::maths::normalize([0, 1, 2].map(|r| m[r] * d[0] + m[4 + r] * d[1] + m[8 + r] * d[2]))
}

/// A node's own transform.
fn local(node: &Value) -> M4 {
    if let Some(m) = node.get("matrix").and_then(Value::floats::<16>) {
        return m;
    }
    let t = node.get("translation").and_then(Value::floats::<3>).unwrap_or([0.0; 3]);
    let [x, y, z, w] = node.get("rotation").and_then(Value::floats::<4>).unwrap_or([0.0, 0.0, 0.0, 1.0]);
    let s = node.get("scale").and_then(Value::floats::<3>).unwrap_or([1.0; 3]);
    let r = [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y + z * w), 2.0 * (x * z - y * w)],
        [2.0 * (x * y - z * w), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z + x * w)],
        [2.0 * (x * z + y * w), 2.0 * (y * z - x * w), 1.0 - 2.0 * (x * x + y * y)],
    ];
    let mut m = IDENTITY;
    for c in 0..3 {
        for row in 0..3 {
            m[c * 4 + row] = r[c][row] * s[c];
        }
    }
    m[12..15].copy_from_slice(&t);
    m
}

/// From the file's axes (`y` up, `-z` front) to view axes (`z` up, `-y` north).
fn to_view([x, y, z]: V3) -> V3 {
    [x, z, y]
}

impl Reader<'_> {
    fn list(&self, name: &str) -> &[Value] {
        self.doc.get(name).map_or(&[], Value::items)
    }

    fn view(&self, index: usize) -> Result<(&[u8], usize), String> {
        let v = self.list("bufferViews").get(index).ok_or("glb: no such buffer view")?;
        let start = v.get("byteOffset").and_then(Value::index).unwrap_or(0);
        let len = v.get("byteLength").and_then(Value::index).ok_or("glb: a buffer view has no length")?;
        let stride = v.get("byteStride").and_then(Value::index).unwrap_or(0);
        Ok((self.bin.get(start..start + len).ok_or("glb: a buffer view runs past the buffer")?, stride))
    }

    /// An accessor's values as floats, `width` to an item (indices come out as whole floats).
    fn accessor(&self, index: usize, width: usize) -> Result<Vec<f32>, String> {
        let a = self.list("accessors").get(index).ok_or("glb: no such accessor")?;
        let count = a.get("count").and_then(Value::index).ok_or("glb: an accessor has no count")?;
        let kind = a.get("componentType").and_then(Value::index).ok_or("glb: an accessor has no type")?;
        let size = match kind {
            5121 => 1,
            5123 => 2,
            5125 | 5126 => 4,
            _ => return Err(format!("glb: component type {kind} is not read")),
        };
        let (data, stride) = self.view(a.get("bufferView").and_then(Value::index).ok_or("glb: a sparse accessor")?)?;
        let offset = a.get("byteOffset").and_then(Value::index).unwrap_or(0);
        let stride = if stride == 0 { size * width } else { stride };
        let mut out = Vec::with_capacity(count * width);
        for i in 0..count {
            for c in 0..width {
                let at = offset + i * stride + c * size;
                let b = data.get(at..at + size).ok_or("glb: an accessor runs past its view")?;
                out.push(match kind {
                    5121 => f32::from(b[0]),
                    5123 => f32::from(u16::from_le_bytes([b[0], b[1]])),
                    5125 => u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32,
                    _ => f32::from_le_bytes([b[0], b[1], b[2], b[3]]),
                });
            }
        }
        Ok(out)
    }

    fn image(&self, index: usize) -> Result<Image, String> {
        let i = self.list("images").get(index).ok_or("glb: no such image")?;
        let (data, _) =
            self.view(i.get("bufferView").and_then(Value::index).ok_or("glb: an image outside the file")?)?;
        let decoder = png::Decoder::new(std::io::Cursor::new(data));
        let mut reader = decoder.read_info().map_err(|e| format!("glb: an image: {e}"))?;
        let mut buf = vec![0; reader.output_buffer_size().ok_or("glb: an image too large")?];
        let info = reader.next_frame(&mut buf).map_err(|e| format!("glb: an image: {e}"))?;
        if info.bit_depth != png::BitDepth::Eight {
            return Err("glb: only 8-bit images are read".into());
        }
        let pixels = &buf[..info.buffer_size()];
        let rgba = match info.color_type {
            png::ColorType::Rgba => pixels.to_vec(),
            png::ColorType::Rgb => pixels.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
            png::ColorType::GrayscaleAlpha => pixels.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
            png::ColorType::Grayscale => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            png::ColorType::Indexed => return Err("glb: indexed images are not read".into()),
        };
        Ok(Image { width: info.width, height: info.height, rgba })
    }

    fn model(&self) -> Result<Model, String> {
        let mut images = Vec::new();
        // Images by the file's index, decoded once; a material with no texture gets a one-pixel image of its colour.
        let mut decoded: Vec<Option<usize>> = vec![None; self.list("images").len()];
        let mut material_image = Vec::new();
        let mut team = Vec::new();
        for m in self.list("materials") {
            let pbr = m.get("pbrMetallicRoughness");
            let texture =
                pbr.and_then(|p| p.get("baseColorTexture")).and_then(|t| t.get("index")).and_then(Value::index);
            let source =
                texture.and_then(|t| self.list("textures").get(t)).and_then(|t| t.get("source")).and_then(Value::index);
            let slot = match source {
                Some(s) if s < decoded.len() => match decoded[s] {
                    Some(slot) => slot,
                    None => {
                        images.push(self.image(s)?);
                        decoded[s] = Some(images.len() - 1);
                        images.len() - 1
                    }
                },
                _ => {
                    let c = pbr.and_then(|p| p.get("baseColorFactor")).and_then(Value::floats::<4>).unwrap_or([1.0; 4]);
                    images.push(Image {
                        width: 1,
                        height: 1,
                        rgba: c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8).to_vec(),
                    });
                    images.len() - 1
                }
            };
            material_image.push(slot);
            team.push(m.get("extras").and_then(|e| e.get("team_paint")).and_then(Value::bool).unwrap_or(false));
        }
        if images.is_empty() {
            images.push(Image { width: 1, height: 1, rgba: vec![255; 4] });
        }

        let nodes = self.list("nodes");
        let scene = self.doc.get("scene").and_then(Value::index).unwrap_or(0);
        let roots: Vec<usize> = match self.list("scenes").get(scene) {
            Some(s) => s.get("nodes").map_or(&[][..], Value::items).iter().filter_map(Value::index).collect(),
            None => (0..nodes.len()).collect(),
        };
        let mut pieces = Vec::new();
        // Walk the tree: (node, its parent's world transform, the turning part it is in, as its pivot).
        let mut stack: Vec<(usize, M4, Option<V3>)> = roots.into_iter().rev().map(|n| (n, IDENTITY, None)).collect();
        let mut seen = vec![false; nodes.len()];
        while let Some((n, parent, turning)) = stack.pop() {
            let node = nodes.get(n).ok_or("glb: no such node")?;
            if std::mem::replace(&mut seen[n], true) {
                return Err("glb: a node is in the tree twice".into());
            }
            let world = mul4(&parent, &local(node));
            let name = node.get("name").and_then(Value::str).unwrap_or("");
            let turning = turning.or_else(|| TURNING.contains(&name).then(|| to_view(point(&world, [0.0; 3]))));
            if let Some(mesh) = node.get("mesh").and_then(Value::index) {
                let mesh = self.list("meshes").get(mesh).ok_or("glb: no such mesh")?;
                for p in mesh.get("primitives").map_or(&[][..], Value::items) {
                    if p.get("mode").and_then(Value::index).unwrap_or(4) != 4 {
                        continue;
                    }
                    let attr = |name: &str| p.get("attributes").and_then(|a| a.get(name)).and_then(Value::index);
                    let pos = self.accessor(attr("POSITION").ok_or("glb: a primitive has no positions")?, 3)?;
                    let count = pos.len() / 3;
                    let normals = match attr("NORMAL") {
                        Some(a) => self.accessor(a, 3)?,
                        None => [0.0, 1.0, 0.0].repeat(count),
                    };
                    let uvs = match attr("TEXCOORD_0") {
                        Some(a) => self.accessor(a, 2)?,
                        None => vec![0.0; count * 2],
                    };
                    if normals.len() != count * 3 || uvs.len() != count * 2 {
                        return Err("glb: a primitive's attributes differ in length".into());
                    }
                    let indices: Vec<u32> = match p.get("indices").and_then(Value::index) {
                        Some(a) => self.accessor(a, 1)?.into_iter().map(|i| i as u32).collect(),
                        None => (0..count as u32).collect(),
                    };
                    if indices.iter().any(|&i| i as usize >= count) || !indices.len().is_multiple_of(3) {
                        return Err("glb: bad indices".into());
                    }
                    let material = p.get("material").and_then(Value::index);
                    let pivot = turning.unwrap_or([0.0; 3]);
                    let is_team = material.and_then(|m| team.get(m)).copied().unwrap_or(false);
                    let vertices = (0..count)
                        .map(|i| {
                            let at = to_view(point(&world, [pos[i * 3], pos[i * 3 + 1], pos[i * 3 + 2]]));
                            Vertex {
                                pos: [at[0] - pivot[0], at[1] - pivot[1], at[2] - pivot[2]],
                                normal: to_view(direction(
                                    &world,
                                    [normals[i * 3], normals[i * 3 + 1], normals[i * 3 + 2]],
                                )),
                                uv: [uvs[i * 2], uvs[i * 2 + 1]],
                                team: f32::from(u8::from(is_team)),
                            }
                        })
                        .collect();
                    let image = material.and_then(|m| material_image.get(m)).copied().unwrap_or(0);
                    pieces.push(Piece { turns: turning.is_some(), pivot, vertices, indices, image });
                }
            }
            for child in node.get("children").map_or(&[][..], Value::items).iter().rev() {
                let child = child.index().ok_or("glb: a bad child")?;
                stack.push((child, world, turning));
            }
        }
        if pieces.is_empty() {
            return Err("glb: no triangles".into());
        }
        let (mut min, mut max) = ([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]);
        for p in &pieces {
            for v in &p.vertices {
                for a in 0..3 {
                    min[a] = min[a].min(v.pos[a] + p.pivot[a]);
                    max[a] = max[a].max(v.pos[a] + p.pivot[a]);
                }
            }
        }
        Ok(Model { pieces, images, min, max })
    }
}

/// A model for each unit kind that has one, and the size they are made at.
#[derive(Clone, Debug, Default)]
pub struct Models {
    /// By unit kind; `None` draws the kind as a box.
    pub by_kind: Vec<Option<Model>>,
    /// How many of the models' metres make one cell.
    pub metres_per_cell: f32,
}

/// Where and how big to draw a model for a shape.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    /// Where the model's origin goes, in view space.
    pub origin: V3,
    /// Its size across and up, in cells per metre.
    pub across: f32,
    pub up: f32,
}

impl Models {
    /// The models for the generic skirmish (`ai3d::skirmish`), built into the program from
    /// `assets/skirmish/models/`.
    pub fn skirmish() -> Models {
        const FILES: [(&str, &[u8]); 6] = [
            ("builder.glb", include_bytes!("../../../assets/skirmish/models/builder.glb")),
            ("generator.glb", include_bytes!("../../../assets/skirmish/models/generator.glb")),
            ("extractor.glb", include_bytes!("../../../assets/skirmish/models/extractor.glb")),
            ("factory.glb", include_bytes!("../../../assets/skirmish/models/factory.glb")),
            ("tank.glb", include_bytes!("../../../assets/skirmish/models/tank.glb")),
            ("artillery.glb", include_bytes!("../../../assets/skirmish/models/artillery.glb")),
        ];
        let list = include_str!("../../../assets/skirmish/models/models.json");
        Models::from_list(list, &ai3d::skirmish::KINDS, |file| FILES.iter().find(|f| f.0 == file).map(|f| f.1.to_vec()))
            .expect("the skirmish's models load")
    }

    /// Models from a list (`models.json`: `metres_per_cell`, and `kinds` naming a file for each kind it draws), for
    /// the kinds named in `kinds` by index, reading each file with `read`.
    pub fn from_list(list: &str, kinds: &[&str], read: impl Fn(&str) -> Option<Vec<u8>>) -> Result<Models, String> {
        let doc = json::parse(list)?;
        let metres_per_cell =
            doc.get("metres_per_cell").and_then(Value::num).filter(|m| *m > 0.0).ok_or("models: no metres_per_cell")?
                as f32;
        let files = doc.get("kinds").ok_or("models: no kinds")?;
        let mut by_kind = Vec::new();
        for kind in kinds {
            by_kind.push(match files.get(kind).and_then(Value::str) {
                Some(file) => {
                    let bytes = read(file).ok_or_else(|| format!("models: no file {file}"))?;
                    Some(Model::from_glb(&bytes).map_err(|e| format!("{file}: {e}"))?)
                }
                None => None,
            });
        }
        for (name, _) in files.fields() {
            if !kinds.contains(&name.as_str()) {
                return Err(format!("models: no unit kind called {name}"));
            }
        }
        Ok(Models { by_kind, metres_per_cell })
    }

    /// Where to draw a unit's model over its shape, or `None` if its kind has no model (or the shape is no unit). Every model is drawn at the same size per metre, except that
    /// a building shrinks to fit inside its footprint; a frame rises from the ground as it is built.
    pub fn placement(&self, shape: &Shape) -> Option<Placement> {
        let Shape { min, max, unit: Some(Pose { kind, structure, grown, .. }), .. } = *shape else { return None };
        let m = self.by_kind.get(kind)?.as_ref()?;
        let mut across = 1.0 / self.metres_per_cell;
        if structure {
            let fit = |a: usize| (max[a] - min[a]) / (m.max[a] - m.min[a]).max(1e-3);
            across = across.min(fit(0)).min(fit(1));
        }
        let origin = [(min[0] + max[0]) / 2.0, (min[1] + max[1]) / 2.0, min[2]];
        Some(Placement { origin, across, up: across * grown.clamp(0.0, 1.0) })
    }
}

/// A turn of `yaw` radians clockwise seen from above (in view space, where `y` runs south), as a matrix.
fn turn(yaw: f32) -> Mat4 {
    let (s, c) = yaw.sin_cos();
    [[c, s, 0.0, 0.0], [-s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]]
}

fn shift([x, y, z]: V3) -> Mat4 {
    [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [x, y, z, 1.0]]
}

/// The transform from a piece's own space to view space: placed, turned to face `pose.yaw`, sized, and for a
/// turning piece, turned again about its pivot to face `pose.aim`.
pub fn piece_matrix(at: &Placement, pose: &Pose, piece: &Piece) -> Mat4 {
    let size = [[at.across, 0.0, 0.0, 0.0], [0.0, at.across, 0.0, 0.0], [0.0, 0.0, at.up, 0.0], [0.0, 0.0, 0.0, 1.0]];
    let body = mul(&mul(&shift(at.origin), &turn(pose.yaw)), &size);
    if piece.turns { mul(&mul(&body, &shift(piece.pivot)), &turn(pose.aim - pose.yaw)) } else { body }
}

/// An image and each half-size version of it down to one pixel, each pixel the average of the four above it.
pub fn mipmaps(image: &Image) -> Vec<Image> {
    let mut out = vec![image.clone()];
    loop {
        let last = out.last().expect("one at least");
        if last.width == 1 && last.height == 1 {
            return out;
        }
        let (w, h) = ((last.width / 2).max(1), (last.height / 2).max(1));
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                for c in 0..4 {
                    let mut sum = 0u32;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(last.width - 1), (y * 2 + dy).min(last.height - 1));
                        sum += u32::from(last.rgba[((sy * last.width + sx) * 4 + c) as usize]);
                    }
                    rgba.push(((sum + 2) / 4) as u8);
                }
            }
        }
        out.push(Image { width: w, height: h, rgba });
    }
}
