use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
pub(super) enum Prop {
    Bool(bool),
    Integer(i64),
    Real(f64),
    String(String),
    Data(Vec<u8>),
    Array(Vec<Prop>),
    Dict(BTreeMap<String, Prop>),
    Null,
}

impl Prop {
    pub(super) fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Integer(v) => Some(*v as f64),
            Self::Real(v) => Some(*v),
            _ => None,
        }
    }

    pub(super) fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(v) => Some(v),
            _ => None,
        }
    }

    pub(super) fn as_array(&self) -> Option<&[Prop]> {
        match self {
            Self::Array(v) => Some(v),
            _ => None,
        }
    }

    pub(super) fn as_dict(&self) -> Option<&BTreeMap<String, Prop>> {
        match self {
            Self::Dict(v) => Some(v),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct BrushArchiveSettings {
    root: BTreeMap<String, Prop>,
}

impl BrushArchiveSettings {
    pub(super) fn parse(bytes: &[u8]) -> Result<Self, String> {
        let value = plist::Value::from_reader(std::io::Cursor::new(bytes))
            .map_err(|_| "not a Procreate brush archive".to_string())?;
        let archive = value
            .as_dictionary()
            .ok_or_else(|| "not a Procreate brush archive".to_string())?;
        if archive.get("$archiver").and_then(plist::Value::as_string) != Some("NSKeyedArchiver") {
            return Err("not a Procreate brush archive".to_string());
        }
        let objects = archive
            .get("$objects")
            .and_then(plist::Value::as_array)
            .ok_or_else(|| "brush archive has no objects".to_string())?;
        let root_uid = archive
            .get("$top")
            .and_then(plist::Value::as_dictionary)
            .and_then(|top| top.get("root"))
            .and_then(plist::Value::as_uid)
            .ok_or_else(|| "brush archive has no root".to_string())?;
        match resolve_uid(root_uid, objects, 0)? {
            Prop::Dict(root) => Ok(Self { root }),
            _ => Err("brush archive root is not a dictionary".to_string()),
        }
    }

    pub(super) fn get(&self, key: &str) -> Option<&Prop> {
        self.root.get(key)
    }

    pub(super) fn f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(Prop::as_f64)
    }

    pub(super) fn f32(&self, key: &str) -> Option<f32> {
        self.f64(key).map(|v| v as f32)
    }

    pub(super) fn bool(&self, key: &str) -> Option<bool> {
        self.get(key).and_then(Prop::as_bool)
    }

    pub(super) fn string(&self, key: &str) -> Option<&str> {
        self.get(key).and_then(Prop::as_str)
    }

    pub(super) fn path(&self, keys: &[&str]) -> Option<&Prop> {
        let mut current = self.root.get(*keys.first()?)?;
        for key in &keys[1..] {
            current = current.as_dict()?.get(*key)?;
        }
        Some(current)
    }

    pub(super) fn pressure_curve(&self) -> Option<Vec<[f32; 2]>> {
        self.curve("dynamicsPressureOverall")
    }

    pub(super) fn pressure_size_curve(&self) -> Option<Vec<[f32; 2]>> {
        self.curve("dynamicsPressureSizeCurve")
            .or_else(|| self.pressure_curve())
    }

    pub(super) fn pressure_opacity_curve(&self) -> Option<Vec<[f32; 2]>> {
        self.curve("dynamicsPressureOpacityCurve")
            .or_else(|| self.pressure_curve())
    }

    fn curve(&self, key: &str) -> Option<Vec<[f32; 2]>> {
        let points = self.path(&[key, "points"])?;
        let items = match points {
            Prop::Array(items) => items.as_slice(),
            Prop::Dict(dict) => dict.get("NS.objects")?.as_array()?,
            _ => return None,
        };
        let mut curve = Vec::new();
        for item in items {
            let point = match item {
                Prop::Dict(fields) => {
                    let (Some(x), Some(y)) = (
                        fields.get("x").and_then(Prop::as_f64),
                        fields.get("y").and_then(Prop::as_f64),
                    ) else {
                        continue;
                    };
                    Some((x, y))
                }
                Prop::String(text) => parse_curve_point(text),
                _ => None,
            };
            let Some((x, y)) = point else {
                continue;
            };
            if !x.is_finite() || !y.is_finite() {
                continue;
            }
            curve.push([x.clamp(0.0, 1.0) as f32, y.clamp(0.0, 1.0) as f32]);
        }
        curve.sort_by(|a, b| a[0].total_cmp(&b[0]));
        curve.dedup_by(|a, b| (a[0] - b[0]).abs() < 1e-6);
        (curve.len() >= 2).then_some(curve)
    }
}

fn parse_curve_point(text: &str) -> Option<(f64, f64)> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    let (x, y) = inner.split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}

const MAX_UID_DEPTH: usize = 128;

fn resolve_uid(uid: &plist::Uid, objects: &[plist::Value], depth: usize) -> Result<Prop, String> {
    if depth > MAX_UID_DEPTH {
        return Err("brush archive is cyclic or too deep".to_string());
    }
    let value = objects
        .get(uid.get() as usize)
        .ok_or_else(|| "brush archive has a bad object reference".to_string())?;
    resolve_value(value, objects, depth + 1)
}

fn resolve_value(
    value: &plist::Value,
    objects: &[plist::Value],
    depth: usize,
) -> Result<Prop, String> {
    if depth > MAX_UID_DEPTH {
        return Err("brush archive is cyclic or too deep".to_string());
    }
    match value {
        plist::Value::Uid(uid) => resolve_uid(uid, objects, depth + 1),
        plist::Value::Array(values) => Ok(Prop::Array(
            values
                .iter()
                .map(|value| resolve_value(value, objects, depth + 1))
                .collect::<Result<_, _>>()?,
        )),
        plist::Value::Dictionary(dict) => resolve_dict(dict, objects, depth + 1),
        plist::Value::Boolean(value) => Ok(Prop::Bool(*value)),
        plist::Value::Data(value) => Ok(Prop::Data(value.clone())),
        plist::Value::Integer(_) => value
            .as_signed_integer()
            .map(Prop::Integer)
            .ok_or_else(|| "brush archive has an unsupported integer".to_string()),
        plist::Value::Real(value) => Ok(Prop::Real(*value)),
        plist::Value::String(value) if value == "$null" => Ok(Prop::Null),
        plist::Value::String(value) => Ok(Prop::String(value.clone())),
        _ => Err("brush archive has an unsupported value".to_string()),
    }
}

fn resolve_dict(
    dict: &plist::Dictionary,
    objects: &[plist::Value],
    depth: usize,
) -> Result<Prop, String> {
    if let Some(value) = dict.get("NS.bool").or_else(|| dict.get("NS.boolean")) {
        return match resolve_value(value, objects, depth + 1)? {
            Prop::Bool(value) => Ok(Prop::Bool(value)),
            Prop::Integer(value) => Ok(Prop::Bool(value != 0)),
            _ => Err("brush archive has a bad boolean".to_string()),
        };
    }
    if let Some(value) = dict.get("NS.integer").or_else(|| dict.get("NS.int")) {
        return match resolve_value(value, objects, depth + 1)? {
            Prop::Integer(value) => Ok(Prop::Integer(value)),
            Prop::Real(value) if value.fract() == 0.0 => Ok(Prop::Integer(value as i64)),
            Prop::Bool(value) => Ok(Prop::Integer(value as i64)),
            _ => Err("brush archive has a bad integer".to_string()),
        };
    }
    if let Some(value) = dict
        .get("NS.double")
        .or_else(|| dict.get("NS.real"))
        .or_else(|| dict.get("NS.decimal"))
    {
        return match resolve_value(value, objects, depth + 1)? {
            Prop::Real(value) => Ok(Prop::Real(value)),
            Prop::Integer(value) => Ok(Prop::Real(value as f64)),
            _ => Err("brush archive has a bad real".to_string()),
        };
    }
    if let Some(value) = dict.get("NS.string") {
        return match resolve_value(value, objects, depth + 1)? {
            Prop::String(value) => Ok(Prop::String(value)),
            _ => Err("brush archive has a bad string".to_string()),
        };
    }
    if let Some(value) = dict.get("NS.bytes") {
        return match resolve_value(value, objects, depth + 1)? {
            Prop::Data(value) => Ok(Prop::Data(value)),
            _ => Err("brush archive has a bad data value".to_string()),
        };
    }
    if let Some(value) = dict.get("NS.number") {
        return resolve_value(value, objects, depth + 1);
    }
    if dict.contains_key("NS.null") {
        return Ok(Prop::Null);
    }
    if let (Some(keys), Some(values)) = (dict.get("NS.keys"), dict.get("NS.objects")) {
        let Prop::Array(keys) = resolve_value(keys, objects, depth + 1)? else {
            return Err("brush archive has bad dictionary keys".to_string());
        };
        let Prop::Array(values) = resolve_value(values, objects, depth + 1)? else {
            return Err("brush archive has bad dictionary values".to_string());
        };
        if keys.len() != values.len() {
            return Err("brush archive has mismatched dictionary keys".to_string());
        }
        let mut out = BTreeMap::new();
        for (key, value) in keys.into_iter().zip(values) {
            let Prop::String(key) = key else {
                return Err("brush archive has a non-string dictionary key".to_string());
            };
            out.insert(key, value);
        }
        return Ok(Prop::Dict(out));
    }
    if let Some(values) = dict.get("NS.objects") {
        return match resolve_value(values, objects, depth + 1)? {
            Prop::Array(values) => Ok(Prop::Array(values)),
            _ => Err("brush archive has a bad array".to_string()),
        };
    }
    let mut out = BTreeMap::new();
    for (key, value) in dict {
        if key == "$class" || key == "$classname" {
            continue;
        }
        out.insert(key.clone(), resolve_value(value, objects, depth + 1)?);
    }
    Ok(Prop::Dict(out))
}

#[cfg(test)]
mod tests {
    use super::*;
    use plist::{Dictionary, Uid, Value};

    fn dict_value(entries: Vec<(&str, Value)>) -> Value {
        let mut dict = Dictionary::new();
        for (key, value) in entries {
            dict.insert(key.to_string(), value);
        }
        Value::Dictionary(dict)
    }

    fn ns_dict(entries: Vec<(&str, Value)>) -> Value {
        let keys = entries
            .iter()
            .map(|(key, _)| Value::String((*key).to_string()))
            .collect();
        let values = entries.into_iter().map(|(_, value)| value).collect();
        dict_value(vec![
            ("NS.keys", Value::Array(keys)),
            ("NS.objects", Value::Array(values)),
            ("$class", Value::Uid(Uid::new(2))),
        ])
    }

    fn ns_bool(value: bool) -> Value {
        dict_value(vec![
            ("NS.bool", Value::Boolean(value)),
            ("$class", Value::Uid(Uid::new(2))),
        ])
    }

    fn ns_integer(value: i64) -> Value {
        dict_value(vec![
            ("NS.integer", Value::from(value)),
            ("$class", Value::Uid(Uid::new(2))),
        ])
    }

    fn ns_real(value: f64) -> Value {
        dict_value(vec![
            ("NS.double", Value::Real(value)),
            ("$class", Value::Uid(Uid::new(2))),
        ])
    }

    fn archive_bytes(root: Value) -> Vec<u8> {
        let class = dict_value(vec![(
            "$classname",
            Value::String("NSDictionary".to_string()),
        )]);
        let archive = dict_value(vec![
            ("$archiver", Value::String("NSKeyedArchiver".to_string())),
            ("$version", Value::from(100000i64)),
            ("$top", dict_value(vec![("root", Value::Uid(Uid::new(1)))])),
            (
                "$objects",
                Value::Array(vec![Value::String("$null".to_string()), root, class]),
            ),
        ]);
        let mut bytes = Vec::new();
        archive.to_writer_binary(&mut bytes).unwrap();
        bytes
    }

    #[test]
    fn parses_root_and_nested_keyed_archive() {
        let root = ns_dict(vec![
            ("name", Value::String("Ink".to_string())),
            ("plotSpacing", Value::Real(0.00258)),
            ("shapeRoundness", Value::Real(0.75)),
            (
                "dynamicsPressureOverall",
                ns_dict(vec![(
                    "points",
                    Value::Array(vec![
                        ns_dict(vec![("x", Value::Real(0.0)), ("y", Value::Real(0.0))]),
                        ns_dict(vec![("x", Value::Real(1.0)), ("y", Value::Real(0.5))]),
                    ]),
                )]),
            ),
        ]);
        let settings = BrushArchiveSettings::parse(&archive_bytes(root)).unwrap();
        assert_eq!(settings.string("name"), Some("Ink"));
        assert!((settings.f64("plotSpacing").unwrap() - 0.00258).abs() < 1e-9);
        assert!((settings.f32("shapeRoundness").unwrap() - 0.75).abs() < 1e-6);
        let points = settings
            .path(&["dynamicsPressureOverall", "points"])
            .and_then(Prop::as_array)
            .unwrap();
        assert_eq!(points.len(), 2);
        assert_eq!(
            points[1].as_dict().unwrap().get("y").and_then(Prop::as_f64),
            Some(0.5)
        );
        assert_eq!(
            settings.pressure_curve(),
            Some(vec![[0.0, 0.0], [1.0, 0.5]])
        );
    }

    #[test]
    fn resolves_nsnumber_wrappers() {
        let root = ns_dict(vec![
            ("enabled", ns_bool(true)),
            ("count", ns_integer(3)),
            ("amount", ns_real(0.25)),
        ]);
        let settings = BrushArchiveSettings::parse(&archive_bytes(root)).unwrap();
        assert_eq!(settings.bool("enabled"), Some(true));
        assert_eq!(settings.f64("count"), Some(3.0));
        assert_eq!(settings.f64("amount"), Some(0.25));
    }

    #[test]
    fn parses_string_point_curves_and_separates_size_opacity() {
        let root = ns_dict(vec![
            (
                "dynamicsPressureSizeCurve",
                ns_dict(vec![(
                    "points",
                    Value::Array(vec![
                        Value::String("{0.000000, 0.000000}".to_string()),
                        Value::String("{0.500000, 0.200000}".to_string()),
                        Value::String("{1.000000, 1.000000}".to_string()),
                    ]),
                )]),
            ),
            (
                "dynamicsPressureOpacityCurve",
                ns_dict(vec![(
                    "points",
                    Value::Array(vec![
                        Value::String("{0.000000, 1.000000}".to_string()),
                        Value::String("{1.000000, 0.000000}".to_string()),
                    ]),
                )]),
            ),
        ]);
        let settings = BrushArchiveSettings::parse(&archive_bytes(root)).unwrap();
        assert_eq!(
            settings.pressure_size_curve(),
            Some(vec![[0.0, 0.0], [0.5, 0.2], [1.0, 1.0]])
        );
        assert_eq!(
            settings.pressure_opacity_curve(),
            Some(vec![[0.0, 1.0], [1.0, 0.0]])
        );
    }

    #[test]
    fn rejects_plain_plists_and_truncated_archives() {
        let mut plain = Vec::new();
        Value::from(7i64).to_writer_binary(&mut plain).unwrap();
        assert!(BrushArchiveSettings::parse(&plain).is_err());
        assert!(BrushArchiveSettings::parse(b"not a plist").is_err());
    }

    fn png_of(px: [u8; 4], w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba(px));
        let mut buf = Vec::new();
        use image::ImageEncoder;
        image::codecs::png::PngEncoder::new(&mut buf)
            .write_image(&img, w, h, image::ExtendedColorType::Rgba8)
            .unwrap();
        buf
    }

    fn zip_with(entries: &[(&str, Vec<u8>)]) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let cursor = std::io::Cursor::new(&mut buf);
            let mut zip = zip::ZipWriter::new(cursor);
            for (name, data) in entries {
                zip.start_file(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                use std::io::Write;
                zip.write_all(data).unwrap();
            }
            zip.finish().unwrap();
        }
        buf
    }

    #[test]
    fn archive_settings_flow_into_imported_brush() {
        let shape = png_of([255, 255, 255, 255], 6, 6);
        let archive = archive_bytes(ns_dict(vec![
            ("name", Value::String("Archive Ink".to_string())),
            ("plotSpacing", Value::Real(0.00258)),
            ("paintOpacity", Value::Real(0.5)),
            ("shapeRoundness", Value::Real(0.25)),
            ("jitterShapeRoundness", Value::Real(0.4)),
            ("jitterShapeRoundnessX", Value::Real(0.7)),
            ("shapeRotation", Value::Real(0.5)),
            ("shapeScatter", Value::Real(0.1)),
            ("shapeCount", Value::Real(0.25)),
            ("shapeCountJitter", Value::Real(0.5)),
            ("wetEdgesAmount", Value::Real(0.3)),
            ("grainDepth", Value::Real(0.4)),
            ("dynamicsJitterSize", Value::Real(0.2)),
            ("dynamicsJitterOpacity", Value::Real(0.6)),
            ("dynamicsPressureSize", Value::Real(0.6)),
            ("dynamicsPressureOpacity", Value::Real(-0.3)),
            (
                "dynamicsPressureOverall",
                ns_dict(vec![(
                    "points",
                    ns_dict(vec![(
                        "NS.objects",
                        Value::Array(vec![
                            ns_dict(vec![("x", Value::Real(0.0)), ("y", Value::Real(0.0))]),
                            ns_dict(vec![("x", Value::Real(1.0)), ("y", Value::Real(0.8))]),
                        ]),
                    )]),
                )]),
            ),
        ]));
        let bytes = zip_with(&[
            ("Ink/Shape.png", shape.clone()),
            ("Ink/Grain.png", shape),
            ("Ink/Brush.archive", archive),
        ]);
        let list = super::super::import_procreate(&bytes, "Fallback").unwrap();
        let brush = &list[0].brush;
        assert_eq!(list[0].name, "Archive Ink");
        assert!((brush.spacing - 0.05).abs() < 0.001);
        assert!((brush.opacity - 0.5).abs() < 0.001);
        assert!((brush.roundness - 0.25).abs() < 0.001);
        assert!((brush.roundness_jitter - 0.7).abs() < 0.001);
        assert!((brush.size_jitter - 0.2).abs() < 0.001);
        assert!((brush.opacity_jitter - 0.6).abs() < 0.001);
        assert!((brush.shape_rotation - 0.5).abs() < 0.001);
        assert!((brush.angle_scatter - 0.05).abs() < 0.001);
        assert_eq!(brush.shape_count, 4);
        assert!((brush.shape_count_jitter - 0.5).abs() < 0.001);
        assert!((brush.wet_edge - 0.3).abs() < 0.001);
        assert!((brush.grain - 0.4).abs() < 0.001);
        assert!((brush.pressure_size - 0.6).abs() < 0.001);
        assert!((brush.pressure_opacity + 0.3).abs() < 0.001);
        assert_eq!(brush.pressure_curve, Some(vec![[0.0, 0.0], [1.0, 0.8]]));
    }

    #[test]
    fn archive_without_dynamics_uses_flat_defaults() {
        let shape = png_of([255, 255, 255, 255], 6, 6);
        let archive = archive_bytes(ns_dict(vec![("name", Value::String("Flat".to_string()))]));
        let bytes = zip_with(&[("Shape.png", shape), ("Brush.archive", archive)]);
        let list = super::super::import_procreate(&bytes, "Fallback").unwrap();
        let brush = &list[0].brush;
        assert_eq!(brush.pressure_size, 0.0);
        assert_eq!(brush.pressure_opacity, 0.0);
        assert_eq!(brush.velocity_size, 0.0);
        assert_eq!(brush.velocity_opacity, 0.0);
        assert_eq!(brush.roundness_jitter, 0.0);
        assert_eq!(brush.size_jitter, 0.0);
        assert_eq!(brush.opacity_jitter, 0.0);
        assert_eq!(brush.pressure_curve, None);
    }

    #[test]
    fn grain_scale_maps_procreate_texture_scale() {
        let shape = png_of([255, 255, 255, 255], 6, 6);
        for (raw, expected) in [(0.0, 0.25), (4.0, 2.125), (16.0, 4.0), (25.0, 4.0)] {
            let archive = archive_bytes(ns_dict(vec![("textureScale", Value::Real(raw))]));
            let bytes = zip_with(&[("Shape.png", shape.clone()), ("Brush.archive", archive)]);
            let list = super::super::import_procreate(&bytes, "Grain").unwrap();
            let scale = list[0].brush.grain_scale;
            assert!(
                (scale - expected).abs() < 0.001,
                "textureScale {raw} -> {scale}, expected {expected}"
            );
        }
    }

    #[test]
    fn taper_settings_map_from_touch_and_pencil_lengths() {
        let shape = png_of([255, 255, 255, 255], 6, 6);
        let archive = archive_bytes(ns_dict(vec![
            ("taperStartLength", Value::Real(0.2)),
            ("taperEndLength", Value::Real(0.3)),
            ("pencilTaperStartLength", Value::Real(0.6)),
            ("pencilTaperEndLength", Value::Real(0.4)),
            ("taperShape", Value::Real(0.0)),
            ("pencilTaperShape", Value::Real(0.7)),
            ("taperOpacity", Value::Real(0.5)),
            ("pencilTaperOpacity", Value::Real(0.2)),
            ("taperSize", Value::Real(0.25)),
            ("pencilTaperSize", Value::Real(0.8)),
            ("taperPressure", Value::Real(0.6)),
        ]));
        let bytes = zip_with(&[("Shape.png", shape), ("Brush.archive", archive)]);
        let list = super::super::import_procreate(&bytes, "Taper").unwrap();
        let taper = list[0].brush.taper;
        assert!((taper.start - 0.6).abs() < 0.001);
        assert!((taper.end - 0.4).abs() < 0.001);
        assert!((taper.shape - 0.7).abs() < 0.001);
        assert!((taper.opacity - 0.5).abs() < 0.001);
        assert!((taper.size - 0.8).abs() < 0.001);
        assert!((taper.pressure - 0.6).abs() < 0.001);
    }

    #[test]
    fn taper_size_defaults_to_full_when_archive_omits_it() {
        let shape = png_of([255, 255, 255, 255], 6, 6);
        let archive = archive_bytes(ns_dict(vec![
            ("taperStartLength", Value::Real(0.2)),
            ("taperEndLength", Value::Real(0.2)),
        ]));
        let bytes = zip_with(&[("Shape.png", shape), ("Brush.archive", archive)]);
        let list = super::super::import_procreate(&bytes, "TaperDefault").unwrap();
        assert!((list[0].brush.taper.size - 1.0).abs() < 0.001);
    }

    #[test]
    fn missing_root_uid_is_an_error() {
        let archive = dict_value(vec![
            ("$archiver", Value::String("NSKeyedArchiver".to_string())),
            ("$version", Value::from(100000i64)),
            ("$top", dict_value(vec![("root", Value::Uid(Uid::new(9)))])),
            (
                "$objects",
                Value::Array(vec![Value::String("$null".to_string())]),
            ),
        ]);
        let mut bytes = Vec::new();
        archive.to_writer_binary(&mut bytes).unwrap();
        assert!(BrushArchiveSettings::parse(&bytes).is_err());
    }
}
