use crate::settings::DataSizeUnit;

pub fn format_bytes(bytes: u64, unit: DataSizeUnit) -> String {
    let (size, units): (f64, [&str; 5]) = match unit {
        DataSizeUnit::Bytes => (bytes as f64, ["B", "KB", "MB", "GB", "TB"]),
        DataSizeUnit::Mibibytes => (bytes as f64, ["B", "KiB", "MiB", "GiB", "TiB"]),
        DataSizeUnit::Bits => (bytes as f64 * 8.0, ["b", "Kb", "Mb", "Gb", "Tb"]),
    };

    let mut size = size;
    let mut unit_index = 0;

    while size >= 1024.0 && unit_index < units.len() - 1 {
        size /= 1024.0;
        unit_index += 1;
    }

    if unit_index == 0 {
        format!("{} {}", size as u64, units[unit_index])
    } else {
        format!("{:.2} {}", size, units[unit_index])
    }
}

/// Fits `(w, h)` into `(max_w, max_h)` preserving aspect ratio, then grows the result back up
/// (still preserving aspect ratio) if needed so the width is at least `min_w` - e.g. so an
/// overlaid label has room to fit. If `max_h` doesn't allow reaching `min_w` without exceeding
/// it, `max_h` wins and the result stays narrower than `min_w`.
pub fn fit_dimensions(w: f32, h: f32, max_w: f32, max_h: f32, min_w: f32) -> (f32, f32) {
    if w == 0.0 || h == 0.0 {
        return (max_w, max_h);
    }

    let scale = (max_w / w).min(max_h / h).min(1.0);
    let (mut width, mut height) = (w * scale, h * scale);

    if width < min_w {
        let grow = (min_w / width).min(max_h / height);
        width *= grow;
        height *= grow;
    }

    (width.min(max_w), height)
}
