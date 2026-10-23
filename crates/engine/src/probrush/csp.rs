//! Clip Studio Paint `.sut` import. A `.sut` is a SQLite database: `Node`
//! names the brush, `Variant` holds its settings and `MaterialFile` carries
//! tip imagery (usually a tar of PNGs). Only the settings that map cleanly
//! onto a ProCraft brush are imported. Pressure curves from the size and
//! opacity effectors are imported when CSP marks pressure as active. CSP's
//! second effector curve is tilt, not speed, so it is left unmapped along with
//! taper and the remaining effector curves.

use super::{
    cap_list, clamp_spacing, clean_name, decode_png_mask, fit_tip, stamp_brush, Brush, BrushTip,
    ImportedBrush,
};

const MAX_ROW_BYTES: usize = 64 * 1024 * 1024;
const MAX_PNG_CANDIDATES: usize = 32;
const PNG_SIG: [u8; 8] = [0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a];

pub(super) fn import_sut(bytes: &[u8], stem: &str) -> Result<Vec<ImportedBrush>, String> {
    if bytes.len() < 100 || &bytes[..16] != b"SQLite format 3\0" {
        return Err("not a Clip Studio brush file".to_string());
    }
    let db = Db::parse(bytes).ok_or_else(|| "CSP brush database is unreadable".to_string())?;
    let node = db.table("Node");
    let variant = db.table("Variant");

    let mut name = clean_name(stem, "CSP brush");
    let mut node_variant = 0i64;
    if let Some(table) = node.as_ref() {
        if let Some(row) = table.rows.first() {
            if let Some(text) = text_value(table, row, "NodeName") {
                name = clean_name(text, &name);
            }
            node_variant = int_value(table, row, "NodeVariantID").unwrap_or(0);
        }
    }
    let variant_row = variant.as_ref().and_then(|table| {
        table
            .rows
            .iter()
            .find(|row| int_value(table, row, "VariantID") == Some(node_variant))
            .or_else(|| table.rows.first())
    });
    let (settings, pattern_key) = variant
        .as_ref()
        .zip(variant_row)
        .map(|(table, row)| {
            (
                Settings::read(table, row),
                image_reference(table, row, "BrushPatternImageArray"),
            )
        })
        .unwrap_or_default();

    let mut brush = if let Some((w, h, mask)) = material_image(&db, pattern_key.as_deref()) {
        stamp_brush(name.clone(), w, h, mask, settings.spacing)?.brush
    } else {
        Brush {
            size: settings.size,
            opacity: settings.opacity,
            softness: settings.softness,
            spacing: settings.spacing,
            roundness: settings.roundness,
            scatter: settings.scatter,
            tip: None,
            ..Brush::default()
        }
    };
    if let Some(size) = settings.size_override {
        brush.size = size;
    }
    brush.opacity = settings.opacity;
    brush.softness = settings.softness;
    brush.spacing = settings.spacing;
    brush.roundness = settings.roundness;
    brush.scatter = settings.scatter;
    brush.angle = settings.angle;
    brush.angle_scatter = settings.angle_scatter;
    brush.wet_edge = settings.wet_edge;
    if let Some(curve) = settings.pressure_curve {
        brush.pressure_curve = Some(curve);
        brush.pressure_size = 1.0;
    }
    if let Some(curve) = settings.pressure_opacity_curve {
        brush.pressure_opacity_curve = Some(curve);
        brush.pressure_opacity = 1.0;
    }
    if let (Some(table), Some(row)) = (variant.as_ref(), variant_row) {
        apply_texture(&mut brush, &db, table, row);
        apply_dual(&mut brush, &db, table, row);
    }
    cap_list(vec![ImportedBrush { name, brush }])
}

struct Settings {
    size: f32,
    size_override: Option<f32>,
    opacity: f32,
    softness: f32,
    spacing: f32,
    roundness: f32,
    scatter: f32,
    angle: f32,
    angle_scatter: f32,
    wet_edge: f32,
    pressure_curve: Option<Vec<[f32; 2]>>,
    pressure_opacity_curve: Option<Vec<[f32; 2]>>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            size: 24.0,
            size_override: None,
            opacity: 1.0,
            softness: 0.5,
            spacing: 0.15,
            roundness: 1.0,
            scatter: 0.0,
            angle: 0.0,
            angle_scatter: 0.0,
            wet_edge: 0.0,
            pressure_curve: None,
            pressure_opacity_curve: None,
        }
    }
}

impl Settings {
    fn read(table: &Table, row: &[Value]) -> Self {
        let mut s = Settings::default();
        if let Some(v) = number(table, row, "BrushSize") {
            if v.is_finite() && (1.0..=400.0).contains(&v) {
                s.size = v as f32;
                s.size_override = Some(v as f32);
            }
        }
        if let Some(v) = number(table, row, "Opacity") {
            let f = if v > 1.0 { v / 100.0 } else { v };
            if f.is_finite() {
                s.opacity = f.clamp(0.0, 1.0) as f32;
            }
        }
        if let Some(v) = number(table, row, "BrushFlow") {
            let f = if v > 1.0 { v / 100.0 } else { v };
            if f.is_finite() {
                s.opacity = (s.opacity * f.clamp(0.0, 1.0) as f32).clamp(0.0, 1.0);
            }
        }
        if let Some(v) = number(table, row, "BrushHardness") {
            if (0.0..=100.0).contains(&v) {
                s.softness = (1.0 - v / 100.0) as f32;
            }
        }
        if let Some(v) = number(table, row, "BrushInterval") {
            if v.is_finite() && v > 0.0 {
                s.spacing = clamp_spacing((v / 100.0) as f32, 0.15);
            }
        }
        if let Some(v) = number(table, row, "BrushThickness") {
            if (1.0..=100.0).contains(&v) {
                s.roundness = (v / 100.0).clamp(0.1, 1.0) as f32;
            }
        }
        if int_value(table, row, "BrushUseSpray").unwrap_or(0) != 0 {
            let size = number(table, row, "BrushSize").unwrap_or(24.0).max(1.0);
            let spray = number(table, row, "BrushSpraySize").unwrap_or(size * 0.3);
            s.scatter = (spray / size).clamp(0.0, 1.0) as f32;
        }
        if let Some(v) = number(table, row, "BrushRotation") {
            if v.is_finite() {
                s.angle = v.to_radians() as f32;
            }
        }
        if let Some(v) = number(table, row, "BrushRotationRandomScale") {
            if v.is_finite() {
                s.angle_scatter = (v / 100.0).clamp(0.0, 1.0) as f32;
            }
        }
        if int_value(table, row, "BrushUseWaterEdge").unwrap_or(0) != 0 {
            s.wet_edge = 1.0;
        }
        s.pressure_curve = effector_pressure_curve(table, row, "BrushSizeEffector");
        s.pressure_opacity_curve = effector_pressure_curve(table, row, "BrushOpacityEffector");
        s
    }
}

fn effector_pressure_curve(table: &Table, row: &[Value], name: &str) -> Option<Vec<[f32; 2]>> {
    let Value::Blob(blob) = value(table, row, name)? else {
        return None;
    };
    pressure_curve_from_effector(blob)
}

fn pressure_curve_from_effector(blob: &[u8]) -> Option<Vec<[f32; 2]>> {
    const PRESSURE: u32 = 0x10;
    const CURVE_KIND: u32 = 12;
    const CURVE_STRIDE: usize = 16;

    let header = be32(blob, 0)? as usize;
    if !(40..=48).contains(&header) || !header.is_multiple_of(4) || header > blob.len() {
        return None;
    }
    if be32(blob, 8)? & PRESSURE == 0 {
        return None;
    }
    let slots = (header == 44 || header == 48)
        .then(|| Some([be32(blob, 32)? as usize, be32(blob, 36)? as usize]))
        .flatten();

    let mut curves = Vec::new();
    let mut offset = header;
    while curves.len() < 4 && offset.checked_add(12)? <= blob.len() {
        let kind = be32(blob, offset)?;
        let count = be32(blob, offset + 4)? as usize;
        let stride = be32(blob, offset + 8)? as usize;
        if kind != CURVE_KIND || stride != CURVE_STRIDE || !(2..=64).contains(&count) {
            break;
        }
        let start = offset.checked_add(12)?;
        let end = start.checked_add(count.checked_mul(stride)?)?;
        if end > blob.len() {
            break;
        }
        let mut curve = Vec::with_capacity(count);
        for point in 0..count {
            let at = start + point * stride;
            let x = f64::from_be_bytes(blob.get(at..at + 8)?.try_into().ok()?);
            let y = f64::from_be_bytes(blob.get(at + 8..at + 16)?.try_into().ok()?);
            if !x.is_finite() || !y.is_finite() {
                return None;
            }
            curve.push([x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32]);
        }
        curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
        curve.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
        if curve.len() >= 2 {
            curves.push(curve);
        }
        offset = end;
    }

    let curve = match slots {
        Some([pressure, tilt]) => {
            let mut next = 0usize;
            let mut pressure_index = None;
            for (slot, bytes) in [pressure, tilt].into_iter().enumerate() {
                if bytes == 0 {
                    continue;
                }
                let curve = curves.get(next)?;
                if 12 + CURVE_STRIDE * curve.len() != bytes {
                    return None;
                }
                if slot == 0 {
                    pressure_index = Some(next);
                }
                next += 1;
            }
            pressure_index.and_then(|index| curves.get(index).cloned())
        }
        None => curves.first().cloned(),
    };
    curve
}

fn image_reference(table: &Table, row: &[Value], column: &str) -> Option<String> {
    let Value::Blob(blob) = value(table, row, column)? else {
        return None;
    };
    utf16_strings(blob)
        .into_iter()
        .find(|text| text.contains(":data:") && text.len() >= 8)
}

fn apply_texture(brush: &mut Brush, db: &Db, table: &Table, row: &[Value]) {
    let Some(key) = image_reference(table, row, "TextureImage") else {
        return;
    };
    let Some((w, h, mask)) = material_image(db, Some(&key)) else {
        return;
    };
    let density = number(table, row, "TextureDensity")
        .unwrap_or(0.0)
        .clamp(0.0, 100.0) as f32
        / 100.0;
    if density <= 0.0 {
        return;
    }
    let (tw, th, mut mask) = fit_tip(w, h, mask);
    if int_value(table, row, "TextureReverseDensity").unwrap_or(0) != 0 {
        for value in &mut mask {
            *value = 255 - *value;
        }
    }
    let Ok(tip) = BrushTip::from_mask(tw, th, mask) else {
        return;
    };
    let original = w.max(h) as f32;
    let resized = tw.max(th) as f32;
    let scale = number(table, row, "TextureScale2").unwrap_or(100.0) as f32 / 100.0;
    let angle = number(table, row, "TextureRotate").unwrap_or(0.0) as f32;
    brush.grain = density;
    brush.grain_tip = Some(tip);
    brush.grain_scale = (original / resized * scale).clamp(0.25, 4.0);
    brush.grain_angle = angle.to_radians();
}

fn apply_dual(brush: &mut Brush, db: &Db, table: &Table, row: &[Value]) {
    if int_value(table, row, "UseDualBrush").unwrap_or(0) == 0 {
        return;
    }
    let Some(key) = image_reference(table, row, "DualPatternImageArray") else {
        return;
    };
    let Some((w, h, mask)) = material_image(db, Some(&key)) else {
        return;
    };
    let (tw, th, mask) = fit_tip(w, h, mask);
    let Ok(tip) = BrushTip::from_mask(tw, th, mask) else {
        return;
    };
    let main = number(table, row, "BrushSize")
        .unwrap_or(brush.size as f64)
        .max(1.0);
    let dual = number(table, row, "DualSize").unwrap_or(main);
    brush.dual_tip = Some(tip);
    brush.dual_scale = (dual / main).clamp(0.1, 4.0) as f32;
}

fn utf16_strings(bytes: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for pair in bytes.as_chunks::<2>().0 {
        let code = u16::from_le_bytes(*pair);
        if (0x20..=0x7e).contains(&code) {
            current.push(code as u8 as char);
        } else {
            if current.len() >= 4 {
                out.push(std::mem::take(&mut current));
            } else {
                current.clear();
            }
        }
    }
    if current.len() >= 4 {
        out.push(current);
    }
    out
}

fn row_contains_text(row: &[Value], needle: &str) -> bool {
    row.iter()
        .any(|value| matches!(value, Value::Text(text) if text.contains(needle)))
}

fn row_contains_tag(row: &[Value], tag: &[u8]) -> bool {
    row.iter()
        .any(|value| matches!(value, Value::Blob(blob) if find_bytes(blob, 0, tag).is_some()))
}

fn material_image(db: &Db, pattern_key: Option<&str>) -> Option<(u32, u32, Vec<u8>)> {
    let table = db.table("MaterialFile")?;
    let file_column = column_index(&table, "FileData");
    let mut rows: Vec<&Vec<Value>> = Vec::new();
    if let Some(key) = pattern_key {
        rows.extend(table.rows.iter().filter(|row| row_contains_text(row, key)));
    }
    if rows.is_empty() {
        rows.extend(
            table
                .rows
                .iter()
                .filter(|row| row_contains_tag(row, b"BrushPattern")),
        );
    }
    if rows.is_empty() {
        rows.extend(table.rows.iter());
    }

    let mut best: Option<(u64, u32, u32, Vec<u8>)> = None;
    for row in rows {
        if let Some(index) = file_column {
            if let Some(Value::Blob(blob)) = row.get(index) {
                keep_largest(&mut best, largest_png(blob));
            }
        } else {
            for value in row {
                if let Value::Blob(blob) = value {
                    keep_largest(&mut best, largest_png(blob));
                }
            }
        }
    }
    best.map(|(_, w, h, mask)| (w, h, mask))
}

fn keep_largest(best: &mut Option<(u64, u32, u32, Vec<u8>)>, tip: Option<(u32, u32, Vec<u8>)>) {
    let Some((w, h, mask)) = tip else {
        return;
    };
    let area = w as u64 * h as u64;
    if best.as_ref().map(|b| area > b.0).unwrap_or(true) {
        *best = Some((area, w, h, mask));
    }
}

fn largest_png(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let mut best: Option<(u64, u32, u32, Vec<u8>)> = None;
    let mut from = 0usize;
    for _ in 0..MAX_PNG_CANDIDATES {
        let start = match find_bytes(bytes, from, &PNG_SIG) {
            Some(i) => i,
            None => break,
        };
        let Some(iend) = find_bytes(bytes, start + PNG_SIG.len(), b"IEND") else {
            break;
        };
        let end = iend.checked_add(8)?;
        if end > bytes.len() {
            break;
        }
        let png = &bytes[start..end];
        if let Some((w, h, mask)) = decode_tip_png(png) {
            let area = w as u64 * h as u64;
            if mask.iter().any(|&value| value != 0)
                && best.as_ref().map(|b| area > b.0).unwrap_or(true)
            {
                best = Some((area, w, h, mask));
            }
        }
        from = end;
    }
    best.map(|(_, w, h, mask)| (w, h, mask))
}

fn decode_tip_png(png: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let white = decode_png_mask(png, true).ok()?;
    if white.2.iter().any(|&value| value != 0) {
        return Some(white);
    }
    decode_png_mask(png, false).ok()
}

fn find_bytes(buf: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    let tail = buf.get(from..)?;
    tail.windows(needle.len())
        .position(|window| window == needle)
        .map(|p| p + from)
}

#[derive(Debug)]
enum Value {
    Null,
    Int(i64),
    Real(f64),
    Text(String),
    Blob(Vec<u8>),
}

struct Table {
    columns: Vec<String>,
    rows: Vec<Vec<Value>>,
}

struct SchemaRow {
    name: String,
    root: usize,
    sql: String,
}

struct Db<'a> {
    buf: &'a [u8],
    page_size: usize,
    usable: usize,
}

impl<'a> Db<'a> {
    fn parse(buf: &'a [u8]) -> Option<Self> {
        if buf.len() < 100 {
            return None;
        }
        let raw = u16::from_be_bytes([buf[16], buf[17]]);
        let page_size = if raw == 1 { 65536 } else { raw as usize };
        if !(512..=65536).contains(&page_size) || !page_size.is_power_of_two() {
            return None;
        }
        let usable = page_size.checked_sub(*buf.get(20)? as usize)?;
        if usable < 480 || buf.len() < page_size {
            return None;
        }
        Some(Self {
            buf,
            page_size,
            usable,
        })
    }

    fn page_start(&self, page: usize) -> Option<usize> {
        if page == 0 {
            return None;
        }
        let off = page.checked_sub(1)?.checked_mul(self.page_size)?;
        if off.checked_add(self.page_size)? > self.buf.len() {
            return None;
        }
        Some(off)
    }

    fn page_header(&self, page: usize) -> Option<(usize, u8, usize, usize)> {
        let base = self.page_start(page)?;
        let h = base + if page == 1 { 100 } else { 0 };
        let kind = *self.buf.get(h)?;
        let count = be16(self.buf, h + 3)? as usize;
        let header = match kind {
            0x0d => 8,
            0x05 => 12,
            _ => return None,
        };
        if h.checked_add(header)?.checked_add(count.checked_mul(2)?)? > self.buf.len() {
            return None;
        }
        Some((base, kind, count, h + header))
    }

    fn page_rightmost(&self, page: usize) -> Option<usize> {
        let base = self.page_start(page)?;
        let h = base + if page == 1 { 100 } else { 0 };
        if *self.buf.get(h)? != 0x05 {
            return None;
        }
        be32(self.buf, h + 8).map(|p| p as usize)
    }

    fn walk(&self, page: usize, depth: usize, out: &mut Vec<Vec<Value>>) -> Option<()> {
        if depth > 32 {
            return None;
        }
        let (base, kind, count, ptrs) = self.page_header(page)?;
        for i in 0..count {
            let ptr = be16(self.buf, ptrs.checked_add(i.checked_mul(2)?)?)? as usize;
            let cell = base.checked_add(ptr)?;
            match kind {
                0x0d => {
                    let mut pos = cell;
                    let len = varint(self.buf, &mut pos)?;
                    let _rowid = varint(self.buf, &mut pos)?;
                    let payload = self.payload(pos, len)?;
                    out.push(parse_record(&payload)?);
                }
                0x05 => {
                    let child = be32(self.buf, cell)? as usize;
                    self.walk(child, depth + 1, out)?;
                }
                _ => return None,
            }
        }
        if kind == 0x05 {
            self.walk(self.page_rightmost(page)?, depth + 1, out)?;
        }
        Some(())
    }

    fn payload(&self, start: usize, len: u64) -> Option<Vec<u8>> {
        let len = usize::try_from(len).ok()?;
        if len > MAX_ROW_BYTES {
            return None;
        }
        let max_local = self.usable - 35;
        let min_local = ((self.usable - 12) * 32 / 255).saturating_sub(23);
        let local = if len <= max_local {
            len
        } else {
            let k = min_local + (len - min_local) % (self.usable - 4);
            if k > max_local {
                min_local
            } else {
                k
            }
        };
        if local > len {
            return None;
        }
        let mut out = Vec::with_capacity(len);
        out.extend_from_slice(self.buf.get(start..start.checked_add(local)?)?);
        let mut next = if len > local {
            be32(self.buf, start.checked_add(local)?)? as usize
        } else {
            0
        };
        while out.len() < len {
            if next == 0 {
                return None;
            }
            let base = self.page_start(next)?;
            let take = (len - out.len()).min(self.usable - 4);
            out.extend_from_slice(self.buf.get(base + 4..base + 4 + take)?);
            next = be32(self.buf, base)? as usize;
        }
        Some(out)
    }

    fn schema(&self) -> Option<Vec<SchemaRow>> {
        let mut rows = Vec::new();
        self.walk(1, 0, &mut rows)?;
        let mut out = Vec::new();
        for row in rows {
            let Some(Value::Text(kind)) = row.first() else {
                continue;
            };
            if kind != "table" {
                continue;
            }
            let Some(Value::Text(name)) = row.get(1) else {
                continue;
            };
            let Some(Value::Int(root)) = row.get(3) else {
                continue;
            };
            let Some(Value::Text(sql)) = row.get(4) else {
                continue;
            };
            let root = usize::try_from(*root).ok()?;
            if root == 0 {
                continue;
            }
            out.push(SchemaRow {
                name: name.clone(),
                root,
                sql: sql.clone(),
            });
        }
        Some(out)
    }

    fn table(&self, name: &str) -> Option<Table> {
        let schema = self.schema()?;
        let row = schema
            .into_iter()
            .find(|row| row.name.eq_ignore_ascii_case(name))?;
        let columns = parse_columns(&row.sql)?;
        let mut rows = Vec::new();
        self.walk(row.root, 0, &mut rows)?;
        Some(Table { columns, rows })
    }
}

fn parse_record(payload: &[u8]) -> Option<Vec<Value>> {
    let mut pos = 0usize;
    let header = varint(payload, &mut pos)? as usize;
    if header < pos || header > payload.len() {
        return None;
    }
    let mut serials = Vec::new();
    while pos < header {
        serials.push(varint(payload, &mut pos)?);
    }
    let mut body = header;
    let mut out = Vec::with_capacity(serials.len());
    for serial in serials {
        let len = serial_len(serial)?;
        let bytes = payload.get(body..body.checked_add(len)?)?;
        body += len;
        out.push(match serial {
            0 => Value::Null,
            7 => Value::Real(f64::from_be_bytes(bytes.try_into().ok()?)),
            8 => Value::Int(0),
            9 => Value::Int(1),
            1..=6 => Value::Int(signed_be(bytes)),
            n if n >= 12 => {
                if n % 2 == 1 {
                    Value::Text(String::from_utf8_lossy(bytes).into_owned())
                } else {
                    Value::Blob(bytes.to_vec())
                }
            }
            _ => return None,
        });
    }
    Some(out)
}

fn serial_len(serial: u64) -> Option<usize> {
    Some(match serial {
        0 | 8 | 9 | 10 | 11 => 0,
        1 => 1,
        2 => 2,
        3 => 3,
        4 => 4,
        5 => 6,
        6 | 7 => 8,
        n if n >= 12 => usize::try_from((n - 12) / 2).ok()?,
        _ => return None,
    })
}

fn signed_be(bytes: &[u8]) -> i64 {
    let mut value = 0i64;
    for &byte in bytes {
        value = (value << 8) | byte as i64;
    }
    let bits = bytes.len() * 8;
    if bits < 64 && value & (1i64 << (bits - 1)) != 0 {
        value -= 1i64 << bits;
    }
    value
}

fn varint(buf: &[u8], pos: &mut usize) -> Option<u64> {
    let mut value = 0u64;
    for i in 0..9 {
        let byte = *buf.get(*pos)?;
        *pos += 1;
        if i == 8 {
            return Some((value << 8) | byte as u64);
        }
        value = (value << 7) | (byte & 0x7f) as u64;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

fn be16(buf: &[u8], at: usize) -> Option<u16> {
    let end = at.checked_add(2)?;
    Some(u16::from_be_bytes(buf.get(at..end)?.try_into().ok()?))
}

fn be32(buf: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_be_bytes(buf.get(at..end)?.try_into().ok()?))
}

fn parse_columns(sql: &str) -> Option<Vec<String>> {
    let open = sql.find('(')?;
    let close = sql.rfind(')')?;
    if close <= open {
        return None;
    }
    let mut columns = Vec::new();
    for part in split_top_level(&sql[open + 1..close]) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let upper = part.to_ascii_uppercase();
        if [
            "PRIMARY",
            "UNIQUE",
            "CHECK",
            "FOREIGN",
            "CONSTRAINT",
            "WITHOUT",
        ]
        .iter()
        .any(|word| upper.starts_with(word))
        {
            continue;
        }
        if let Some(name) = parse_ident(part) {
            columns.push(name);
        }
    }
    if columns.is_empty() {
        None
    } else {
        Some(columns)
    }
}

fn split_top_level(body: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut bracket = false;
    let mut chars = body.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if let Some(q) = quote {
            if c == q {
                if chars.peek().map(|(_, n)| *n == q).unwrap_or(false) {
                    chars.next();
                } else {
                    quote = None;
                }
            }
            continue;
        }
        if bracket {
            if c == ']' {
                bracket = false;
            }
            continue;
        }
        match c {
            '\'' | '"' | '`' => quote = Some(c),
            '[' => bracket = true,
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(&body[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&body[start..]);
    parts
}

fn parse_ident(definition: &str) -> Option<String> {
    let text = definition.trim_start();
    let first = text.chars().next()?;
    if first == '[' {
        let end = text.find(']')?;
        let name = text[1..end].trim();
        return (!name.is_empty()).then(|| name.to_string());
    }
    if first == '"' || first == '`' || first == '\'' {
        let mut chars = text.char_indices();
        chars.next();
        let mut name = String::new();
        while let Some((_, c)) = chars.next() {
            if c == first {
                if chars
                    .clone()
                    .next()
                    .map(|(_, n)| n == first)
                    .unwrap_or(false)
                {
                    chars.next();
                    name.push(c);
                } else {
                    return (!name.is_empty()).then_some(name);
                }
            } else {
                name.push(c);
            }
        }
        return None;
    }
    let name = text
        .split(|c: char| c.is_whitespace() || c == '(')
        .next()?
        .trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn column_index(table: &Table, name: &str) -> Option<usize> {
    table
        .columns
        .iter()
        .position(|column| column.eq_ignore_ascii_case(name))
}

fn value<'a>(table: &Table, row: &'a [Value], name: &str) -> Option<&'a Value> {
    row.get(column_index(table, name)?)
}

fn text_value<'a>(table: &Table, row: &'a [Value], name: &str) -> Option<&'a str> {
    match value(table, row, name)? {
        Value::Text(text) => Some(text.as_str()),
        _ => None,
    }
}

fn int_value(table: &Table, row: &[Value], name: &str) -> Option<i64> {
    match value(table, row, name)? {
        Value::Int(v) => Some(*v),
        _ => None,
    }
}

fn number(table: &Table, row: &[Value], name: &str) -> Option<f64> {
    match value(table, row, name)? {
        Value::Int(v) => Some(*v as f64),
        Value::Real(v) => Some(*v),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effector_blob(
        header: usize,
        flags: u32,
        slots: [usize; 2],
        curves: &[&[[f32; 2]]],
    ) -> Vec<u8> {
        let mut ints = vec![0u32; header / 4];
        ints[0] = header as u32;
        ints[2] = flags;
        if header >= 44 {
            ints[8] = slots[0] as u32;
            ints[9] = slots[1] as u32;
        }
        let mut blob = Vec::new();
        for word in ints {
            blob.extend_from_slice(&word.to_be_bytes());
        }
        for curve in curves {
            blob.extend_from_slice(&12u32.to_be_bytes());
            blob.extend_from_slice(&(curve.len() as u32).to_be_bytes());
            blob.extend_from_slice(&16u32.to_be_bytes());
            for point in *curve {
                blob.extend_from_slice(&(point[0] as f64).to_be_bytes());
                blob.extend_from_slice(&(point[1] as f64).to_be_bytes());
            }
        }
        blob
    }

    #[test]
    fn effector_uses_the_pressure_slot_not_the_tilt_slot() {
        let pressure = [[0.0, 0.0], [1.0, 1.0]];
        let tilt = [[0.0, 1.0], [1.0, 0.0]];
        let blob = effector_blob(48, 0x30, [44, 44], &[&pressure, &tilt]);
        assert_eq!(
            pressure_curve_from_effector(&blob).as_deref(),
            Some(&pressure[..])
        );
    }

    #[test]
    fn effector_does_not_use_a_tilt_only_curve_as_pressure() {
        let tilt = [[0.0, 1.0], [1.0, 0.0]];
        let blob = effector_blob(48, 0x20, [0, 44], &[&tilt]);
        assert_eq!(pressure_curve_from_effector(&blob), None);
    }

    #[test]
    fn effector_ignores_curves_when_pressure_is_inactive() {
        let pressure = [[0.0, 0.0], [1.0, 1.0]];
        let blob = effector_blob(48, 0x20, [44, 0], &[&pressure]);
        assert_eq!(pressure_curve_from_effector(&blob), None);
    }

    #[test]
    fn effector_reads_a_44_byte_header_and_ignores_extra_speed_curve() {
        let pressure = [[0.0, 0.0], [1.0, 1.0]];
        let speed = [[0.0, 1.0], [1.0, 0.0]];
        let blob = effector_blob(44, 0x50, [44, 0], &[&pressure, &speed]);
        assert_eq!(
            pressure_curve_from_effector(&blob).as_deref(),
            Some(&pressure[..])
        );
    }

    #[test]
    fn sut_reads_name_settings_and_embedded_tip() {
        let bytes = include_bytes!("../../testdata/csp_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        assert_eq!(brushes.len(), 1);
        let imported = &brushes[0];
        assert_eq!(imported.name, "Ink Wash");
        assert!((imported.brush.size - 42.0).abs() < 0.01);
        assert!((imported.brush.opacity - 0.6).abs() < 0.01);
        assert!((imported.brush.softness - 0.8).abs() < 0.01);
        assert!((imported.brush.spacing - 0.15).abs() < 0.01);
        assert!((imported.brush.roundness - 0.8).abs() < 0.01);
        let tip = imported.brush.tip.as_ref().expect("tip");
        assert_eq!((tip.w, tip.h), (8, 8));
        assert!(tip.mask[3 * 8 + 3] > 200);
        assert_eq!(tip.mask[0], 0);
    }

    #[test]
    fn sut_imports_size_pressure_curve() {
        let bytes = include_bytes!("../../testdata/csp_pressure_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        let brush = &brushes[0].brush;
        assert_eq!(brush.pressure_size, 1.0);
        assert_eq!(
            brush.pressure_curve.as_deref(),
            Some(&[[0.0, 0.0], [0.5, 0.35], [1.0, 1.0]][..])
        );
        assert!((brush.resolve_size(0.5, 0.0) - 14.0).abs() < 0.001);
    }

    #[test]
    fn sut_imports_opacity_pressure_curve() {
        let bytes = include_bytes!("../../testdata/csp_pressure_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        let brush = &brushes[0].brush;
        assert_eq!(brush.pressure_opacity, 1.0);
        assert_eq!(
            brush.pressure_opacity_curve.as_deref(),
            Some(&[[0.0, 0.0], [0.25, 0.1], [0.75, 0.8], [1.0, 1.0]][..])
        );
        assert!((brush.resolve_opacity(0.5, 0.0) - 0.45).abs() < 0.001);
    }

    #[test]
    fn sut_does_not_import_tilt_as_velocity() {
        let bytes = include_bytes!("../../testdata/csp_pressure_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        let brush = &brushes[0].brush;
        assert_eq!(brush.velocity_size_curve, None);
        assert_eq!(brush.velocity_opacity_curve, None);
        for speed in [0.25, 0.5, 0.8, 1.0] {
            assert!(brush.resolve_size(1.0, speed) > 0.0);
            assert!(brush.resolve_opacity(1.0, speed) > 0.0);
        }
    }

    #[test]
    fn sut_prefers_pattern_material_over_paper_texture() {
        let bytes = include_bytes!("../../testdata/csp_pattern_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        assert_eq!(brushes.len(), 1);
        let imported = &brushes[0];
        assert_eq!(imported.name, "Synthetic Pattern Brush");
        assert!((imported.brush.size - 60.0).abs() < 0.01);
        assert!((imported.brush.opacity - 0.4).abs() < 0.01);
        assert!((imported.brush.softness - 0.3).abs() < 0.01);
        assert!((imported.brush.spacing - 0.0097).abs() < 0.0001);
        assert!((imported.brush.roundness - 0.3).abs() < 0.01);
        assert!((imported.brush.scatter - 0.5).abs() < 0.01);
        let tip = imported.brush.tip.as_ref().expect("tip");
        assert_eq!((tip.w, tip.h), (16, 16));
        assert!(tip.mask[8 * 16 + 8] > 200);
        assert_eq!(tip.mask[0], 0);
    }

    #[test]
    fn sut_reads_black_alpha_pattern_tip() {
        let bytes = include_bytes!("../../testdata/csp_black_tip_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        let tip = brushes[0].brush.tip.as_ref().expect("tip");
        assert_eq!((tip.w, tip.h), (16, 16));
        assert!(tip.mask[8 * 16 + 8] > 200);
        assert_eq!(tip.mask[0], 0);
    }

    #[test]
    fn sut_imports_texture_image_as_grain() {
        let bytes = include_bytes!("../../testdata/csp_texture_sample.sut");
        let brushes = import_sut(bytes, "sample").expect("import");
        let brush = &brushes[0].brush;
        let grain = brush.grain_tip.as_ref().expect("texture grain");
        assert_eq!((grain.w, grain.h), (8, 8));
        assert_eq!(grain.mask[0], 255);
        assert!(grain.mask[7] < 40);
        assert!((brush.grain - 0.6).abs() < 0.01);
        assert!((brush.grain_scale - 0.5).abs() < 0.01);
        assert!((brush.grain_angle - std::f32::consts::FRAC_PI_2).abs() < 0.001);
        assert!((brush.angle - 30f32.to_radians()).abs() < 0.001);
        assert!((brush.angle_scatter - 0.2).abs() < 0.01);
        assert!((brush.wet_edge - 1.0).abs() < 0.01);
        let dual = brush.dual_tip.as_ref().expect("dual tip");
        assert_eq!((dual.w, dual.h), (16, 16));
        assert!(dual.mask[8 * 16 + 8] > 200);
        assert!((brush.dual_scale - 1.6).abs() < 0.01);
    }

    #[test]
    fn import_brush_file_dispatches_sut() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join("csp_sample.sut");
        let brushes = super::super::import_brush_file(&path).expect("import");
        assert_eq!(brushes.len(), 1);
        assert_eq!(brushes[0].name, "Ink Wash");
    }

    #[test]
    fn sut_rejects_a_non_database_file() {
        let err = import_sut(b"not a sqlite file", "x").unwrap_err();
        assert!(err.contains("Clip Studio"));
    }

    #[test]
    fn columns_ignore_constraints_and_keep_quoted_names() {
        let sql = "CREATE TABLE Variant(_PW_ID INTEGER PRIMARY KEY AUTOINCREMENT, \
                   \"Brush Size\" REAL, name TEXT CHECK(name IN ('a,b', 'c')), \
                   UNIQUE(name), BrushHardness INTEGER)";
        let columns = parse_columns(sql).expect("columns");
        assert_eq!(
            columns,
            vec!["_PW_ID", "Brush Size", "name", "BrushHardness"]
        );
    }
}
