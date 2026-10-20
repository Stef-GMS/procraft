//! Clip Studio Paint `.sut` import. A `.sut` is a SQLite database: `Node`
//! names the brush, `Variant` holds its settings and `MaterialFile` carries
//! tip imagery (usually a tar of PNGs). Only the settings that map cleanly
//! onto a ProCraft brush are imported; CSP dynamics curves are not.

use super::{
    cap_list, clamp_spacing, clean_name, decode_png_mask, stamp_brush, Brush, ImportedBrush,
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
    let settings = variant
        .as_ref()
        .and_then(|table| {
            let row = table
                .rows
                .iter()
                .find(|row| int_value(table, row, "VariantID") == Some(node_variant))
                .or_else(|| table.rows.first())?;
            Some(Settings::read(table, row))
        })
        .unwrap_or_default();

    let mut brush = if let Some((w, h, mask)) = material_tip(&db) {
        stamp_brush(name.clone(), w, h, mask, settings.spacing)?.brush
    } else {
        Brush {
            size: settings.size,
            opacity: settings.opacity,
            softness: settings.softness,
            spacing: settings.spacing,
            roundness: settings.roundness,
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

    cap_list(vec![ImportedBrush { name, brush }])
}

struct Settings {
    size: f32,
    size_override: Option<f32>,
    opacity: f32,
    softness: f32,
    spacing: f32,
    roundness: f32,
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
        if let Some(v) = number(table, row, "BrushHardness") {
            if (0.0..=100.0).contains(&v) {
                s.softness = (1.0 - v / 100.0) as f32;
            }
        }
        if let Some(v) = number(table, row, "BrushInterval") {
            if v.is_finite() && v > 0.0 {
                let f = if v > 2.0 { v / 100.0 } else { v };
                s.spacing = clamp_spacing(f as f32, 0.15);
            }
        }
        if let Some(v) = number(table, row, "BrushThickness") {
            if (1.0..=100.0).contains(&v) {
                s.roundness = (v / 100.0).clamp(0.1, 1.0) as f32;
            }
        }
        s
    }
}

fn material_tip(db: &Db) -> Option<(u32, u32, Vec<u8>)> {
    let table = db.table("MaterialFile")?;
    let file_column = column_index(&table, "FileData");
    let mut best: Option<(u64, u32, u32, Vec<u8>)> = None;
    for row in &table.rows {
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
        if let Ok((w, h, mask)) =
            decode_png_mask(png, true).or_else(|_| decode_png_mask(png, false))
        {
            let area = w as u64 * h as u64;
            if best.as_ref().map(|b| area > b.0).unwrap_or(true) {
                best = Some((area, w, h, mask));
            }
        }
        from = end;
    }
    best.map(|(_, w, h, mask)| (w, h, mask))
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
