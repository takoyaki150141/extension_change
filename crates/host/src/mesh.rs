use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{bail, Context as _, Result};

#[derive(Default)]
struct Mesh {
    vertices: Vec<[f32; 3]>,
    faces: Vec<Vec<u32>>,
}

pub fn convert(input: &Path, output: &Path, target: &str) -> Result<()> {
    let mesh = load(input).with_context(|| format!("cannot read mesh: {}", input.display()))?;
    if mesh.vertices.is_empty() || mesh.faces.is_empty() {
        bail!("mesh has no faces");
    }
    save(&mesh, output, target).with_context(|| format!("cannot write mesh: {}", output.display()))
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn load(path: &Path) -> Result<Mesh> {
    match extension(path).as_str() {
        "obj" => load_obj(path),
        "ply" => load_ply(path),
        "off" => load_off(path),
        other => bail!("unsupported 3D format: {other}"),
    }
}

fn save(mesh: &Mesh, path: &Path, target: &str) -> Result<()> {
    match target {
        "obj" => save_obj(mesh, path),
        "ply" => save_ply(mesh, path),
        "off" => save_off(mesh, path),
        other => bail!("unsupported 3D format: {other}"),
    }
}

fn parse_index(token: &str, count: usize) -> Result<u32> {
    let value: i64 = token
        .split('/')
        .next()
        .unwrap_or("")
        .parse()
        .with_context(|| format!("invalid face index: {token}"))?;
    let index = if value < 0 { count as i64 + value } else { value - 1 };
    if index < 0 || index >= count as i64 {
        bail!("face index out of range: {value}");
    }
    Ok(index as u32)
}

fn load_obj(path: &Path) -> Result<Mesh> {
    let text = fs::read_to_string(path)?;
    let mut mesh = Mesh::default();
    for line in text.lines() {
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("v") => {
                let values: Vec<f32> = parts
                    .take(3)
                    .map(|value| value.parse())
                    .collect::<Result<_, _>>()?;
                if values.len() != 3 {
                    bail!("invalid vertex in OBJ");
                }
                mesh.vertices.push([values[0], values[1], values[2]]);
            }
            Some("f") => {
                let face: Vec<u32> = parts
                    .map(|token| parse_index(token, mesh.vertices.len()))
                    .collect::<Result<_, _>>()?;
                if face.len() >= 3 {
                    mesh.faces.push(face);
                }
            }
            _ => {}
        }
    }
    mesh.faces
        .retain(|face| face.iter().all(|index| (*index as usize) < mesh.vertices.len()));
    Ok(mesh)
}

#[derive(Clone, Copy)]
enum Scalar {
    I8,
    U8,
    I16,
    U16,
    I32,
    U32,
    F32,
    F64,
}

impl Scalar {
    fn parse(value: &str) -> Option<Self> {
        match value {
            "char" | "int8" => Some(Self::I8),
            "uchar" | "uint8" => Some(Self::U8),
            "short" | "int16" => Some(Self::I16),
            "ushort" | "uint16" => Some(Self::U16),
            "int" | "int32" => Some(Self::I32),
            "uint" | "uint32" => Some(Self::U32),
            "float" | "float32" => Some(Self::F32),
            "double" | "float64" => Some(Self::F64),
            _ => None,
        }
    }

    fn size(self) -> usize {
        match self {
            Self::I8 | Self::U8 => 1,
            Self::I16 | Self::U16 => 2,
            Self::I32 | Self::U32 | Self::F32 => 4,
            Self::F64 => 8,
        }
    }

    fn read(self, bytes: &[u8], position: &mut usize) -> Result<f64> {
        let size = self.size();
        if *position + size > bytes.len() {
            bail!("unexpected end of PLY");
        }
        let value = match self {
            Self::I8 => bytes[*position] as i8 as f64,
            Self::U8 => bytes[*position] as f64,
            Self::I16 => {
                i16::from_le_bytes(bytes[*position..*position + 2].try_into().unwrap()) as f64
            }
            Self::U16 => {
                u16::from_le_bytes(bytes[*position..*position + 2].try_into().unwrap()) as f64
            }
            Self::I32 => {
                i32::from_le_bytes(bytes[*position..*position + 4].try_into().unwrap()) as f64
            }
            Self::U32 => {
                u32::from_le_bytes(bytes[*position..*position + 4].try_into().unwrap()) as f64
            }
            Self::F32 => {
                f32::from_le_bytes(bytes[*position..*position + 4].try_into().unwrap()) as f64
            }
            Self::F64 => f64::from_le_bytes(bytes[*position..*position + 8].try_into().unwrap()),
        };
        *position += size;
        Ok(value)
    }

    fn read_text<'a>(self, tokens: &mut impl Iterator<Item = &'a str>) -> Result<f64> {
        let token = tokens
            .next()
            .ok_or_else(|| anyhow::anyhow!("unexpected end of PLY"))?;
        token
            .parse::<f64>()
            .with_context(|| format!("invalid PLY value: {token}"))
    }
}

enum Property {
    Scalar {
        name: String,
        kind: Scalar,
    },
    List {
        name: String,
        count: Scalar,
        item: Scalar,
    },
}

struct Element {
    name: String,
    count: usize,
    properties: Vec<Property>,
}

fn load_ply(path: &Path) -> Result<Mesh> {
    let bytes = fs::read(path)?;
    let marker = b"end_header";
    let header_end = bytes
        .windows(marker.len())
        .position(|window| window == marker)
        .ok_or_else(|| anyhow::anyhow!("PLY header not found"))?;
    let data_start = bytes[header_end..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map(|offset| header_end + offset + 1)
        .ok_or_else(|| anyhow::anyhow!("PLY header is not terminated"))?;
    let header = String::from_utf8_lossy(&bytes[..header_end]);
    let mut lines = header.lines();
    if lines.next().map(str::trim) != Some("ply") {
        bail!("invalid PLY header");
    }

    let mut ascii = false;
    let mut elements = Vec::new();
    let mut current: Option<Element> = None;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        match parts.next() {
            Some("format") => {
                ascii = parts.next() == Some("ascii");
            }
            Some("element") => {
                if let Some(element) = current.take() {
                    elements.push(element);
                }
                let name = parts.next().unwrap_or("").to_string();
                let count = parts.next().unwrap_or("0").parse().unwrap_or(0);
                current = Some(Element {
                    name,
                    count,
                    properties: Vec::new(),
                });
            }
            Some("property") => {
                let Some(element) = current.as_mut() else {
                    continue;
                };
                let first = parts.next().unwrap_or("");
                if first == "list" {
                    let count = Scalar::parse(parts.next().unwrap_or("")).unwrap_or(Scalar::U8);
                    let item = Scalar::parse(parts.next().unwrap_or("")).unwrap_or(Scalar::I32);
                    let name = parts.next().unwrap_or("").to_string();
                    element
                        .properties
                        .push(Property::List { name, count, item });
                } else {
                    let kind = Scalar::parse(first).unwrap_or(Scalar::F32);
                    let name = parts.next().unwrap_or("").to_string();
                    element
                        .properties
                        .push(Property::Scalar { name, kind });
                }
            }
            _ => {}
        }
    }
    if let Some(element) = current {
        elements.push(element);
    }

    let vertex = elements
        .iter()
        .find(|element| element.name == "vertex")
        .ok_or_else(|| anyhow::anyhow!("PLY has no vertex element"))?;
    let face = elements
        .iter()
        .find(|element| element.name == "face")
        .ok_or_else(|| anyhow::anyhow!("PLY has no face element"))?;
    let vertex_props: Vec<(&str, Scalar)> = vertex
        .properties
        .iter()
        .map(|property| match property {
            Property::Scalar { name, kind } => (name.as_str(), *kind),
            Property::List { name, .. } => (name.as_str(), Scalar::F32),
        })
        .collect();
    let x_index = vertex_props.iter().position(|(name, _)| *name == "x");
    let y_index = vertex_props.iter().position(|(name, _)| *name == "y");
    let z_index = vertex_props.iter().position(|(name, _)| *name == "z");
    let face_list = face.properties.iter().find_map(|property| match property {
        Property::List { name, count, item } if name == "vertex_indices" => {
            Some((*count, *item))
        }
        _ => None,
    });
    let (Some(x_index), Some(y_index), Some(z_index), Some(face_list)) =
        (x_index, y_index, z_index, face_list)
    else {
        bail!("PLY vertex or face layout is unsupported");
    };

    let mut mesh = Mesh::default();
    let data = &bytes[data_start..];
    let mut position = 0usize;
    let tokens = String::from_utf8_lossy(data);
    let mut token_iter = tokens.split_whitespace();
    for element in &elements {
        for _ in 0..element.count {
            if element.name == "vertex" {
                let mut x = 0.0;
                let mut y = 0.0;
                let mut z = 0.0;
                for (index, property) in element.properties.iter().enumerate() {
                    match property {
                        Property::Scalar { kind, .. } => {
                            let value = if ascii {
                                kind.read_text(&mut token_iter)?
                            } else {
                                kind.read(data, &mut position)?
                            };
                            if index == x_index {
                                x = value as f32;
                            } else if index == y_index {
                                y = value as f32;
                            } else if index == z_index {
                                z = value as f32;
                            }
                        }
                        Property::List { count, item, .. } => {
                            let count = if ascii {
                                count.read_text(&mut token_iter)? as usize
                            } else {
                                count.read(data, &mut position)? as usize
                            };
                            for _ in 0..count {
                                if ascii {
                                    item.read_text(&mut token_iter)?;
                                } else {
                                    item.read(data, &mut position)?;
                                }
                            }
                        }
                    }
                }
                mesh.vertices.push([x, y, z]);
            } else if element.name == "face" {
                let (count_kind, item_kind) = face_list;
                let count = if ascii {
                    count_kind.read_text(&mut token_iter)?
                } else {
                    count_kind.read(data, &mut position)?
                } as usize;
                let mut face = Vec::new();
                for _ in 0..count {
                    let value = if ascii {
                        item_kind.read_text(&mut token_iter)?
                    } else {
                        item_kind.read(data, &mut position)?
                    } as u32;
                    face.push(value);
                }
                if face.len() >= 3 {
                    mesh.faces.push(face);
                }
            } else {
                for property in &element.properties {
                    match property {
                        Property::Scalar { kind, .. } => {
                            if ascii {
                                kind.read_text(&mut token_iter)?;
                            } else {
                                kind.read(data, &mut position)?;
                            }
                        }
                        Property::List { count, item, .. } => {
                            let count = if ascii {
                                count.read_text(&mut token_iter)? as usize
                            } else {
                                count.read(data, &mut position)? as usize
                            };
                            for _ in 0..count {
                                if ascii {
                                    item.read_text(&mut token_iter)?;
                                } else {
                                    item.read(data, &mut position)?;
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(mesh)
}

fn load_off(path: &Path) -> Result<Mesh> {
    let text = fs::read_to_string(path)?;
    let mut lines = text.lines();
    let first = lines.next().unwrap_or("").trim();
    let mut first_parts = first.split_whitespace();
    if first_parts.next() != Some("OFF") {
        bail!("invalid OFF header");
    }
    let mut counts: Vec<usize> = first_parts
        .take(2)
        .filter_map(|value| value.parse().ok())
        .collect();
    if counts.len() < 2 {
        counts = lines
            .next()
            .unwrap_or("")
            .split_whitespace()
            .take(2)
            .filter_map(|value| value.parse().ok())
            .collect();
    }
    if counts.len() < 2 {
        bail!("invalid OFF counts");
    }

    let mut mesh = Mesh::default();
    for _ in 0..counts[0] {
        let values: Vec<f32> = lines
            .next()
            .unwrap_or("")
            .split_whitespace()
            .take(3)
            .filter_map(|value| value.parse().ok())
            .collect();
        if values.len() == 3 {
            mesh.vertices.push([values[0], values[1], values[2]]);
        }
    }
    for _ in 0..counts[1] {
        let values: Vec<u32> = lines
            .next()
            .unwrap_or("")
            .split_whitespace()
            .map(|value| value.parse().unwrap_or(0))
            .collect();
        if values.len() >= 4 {
            let count = values[0] as usize;
            if count >= 3 && values.len() >= count + 1 {
                mesh.faces.push(values[1..=count].to_vec());
            }
        }
    }
    Ok(mesh)
}

fn write_obj(mesh: &Mesh, path: &Path) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut out = BufWriter::new(file);
    writeln!(out, "# Extension Change")?;
    for [x, y, z] in &mesh.vertices {
        writeln!(out, "v {x} {y} {z}")?;
    }
    for face in &mesh.faces {
        write!(out, "f")?;
        for index in face {
            write!(out, " {}", index + 1)?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn save_obj(mesh: &Mesh, path: &Path) -> Result<()> {
    write_obj(mesh, path)
}

fn save_ply(mesh: &Mesh, path: &Path) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut out = BufWriter::new(file);
    writeln!(out, "ply")?;
    writeln!(out, "format ascii 1.0")?;
    writeln!(out, "element vertex {}", mesh.vertices.len())?;
    writeln!(out, "property float x")?;
    writeln!(out, "property float y")?;
    writeln!(out, "property float z")?;
    writeln!(out, "element face {}", mesh.faces.len())?;
    writeln!(out, "property list uchar int vertex_indices")?;
    writeln!(out, "end_header")?;
    for [x, y, z] in &mesh.vertices {
        writeln!(out, "{x} {y} {z}")?;
    }
    for face in &mesh.faces {
        write!(out, "{}", face.len())?;
        for index in face {
            write!(out, " {index}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}

fn save_off(mesh: &Mesh, path: &Path) -> Result<()> {
    let file = fs::File::create(path)?;
    let mut out = BufWriter::new(file);
    writeln!(out, "OFF")?;
    writeln!(out, "{} {} 0", mesh.vertices.len(), mesh.faces.len())?;
    for [x, y, z] in &mesh.vertices {
        writeln!(out, "{x} {y} {z}")?;
    }
    for face in &mesh.faces {
        write!(out, "{}", face.len())?;
        for index in face {
            write!(out, " {index}")?;
        }
        writeln!(out)?;
    }
    Ok(())
}
