use std::time::SystemTime;

use chrono::{DateTime, Utc};
use chrono_tz::Tz;

use crate::settings::{DataSizeUnit, DateFormat, HourFormat};

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

pub fn format_message_long_date(
    date: SystemTime,
    timezone: Tz,
    hour_format: HourFormat,
    date_format: DateFormat,
) -> String {
    let hour_str = hour_format.date_format();
    let date_str = date_format.date_format();

    let date = DateTime::<Utc>::from(date).with_timezone(&timezone);
    let now = DateTime::<Utc>::from(SystemTime::now()).with_timezone(&timezone);

    match (date.date_naive() - now.date_naive()).num_days() {
        0 => date.format(&format!("Today, {}", hour_str)).to_string(),
        -1 => date.format(&format!("Yesterday, {}", hour_str)).to_string(),
        -6..-1 => date
            .format(&format!("%a {}, {}", date_str, hour_str))
            .to_string(),
        _ => date
            .format(&format!("{}, {}", date_str, hour_str))
            .to_string(),
    }
}

pub fn format_date_divider(date: SystemTime, timezone: Tz) -> String {
    let now = DateTime::<Utc>::from(SystemTime::now()).with_timezone(&timezone);
    let date = DateTime::<Utc>::from(date).with_timezone(&timezone);

    let is_today = date.date_naive() == now.date_naive();
    let is_yesterday = date.date_naive() == (now - chrono::Duration::days(1)).date_naive();
    let is_week_ago = date > now - chrono::Duration::days(7);

    if is_today {
        "Today".to_string()
    } else if is_yesterday {
        "Yesterday".to_string()
    } else {
        date.format(&format!("{}%d %B %Y", if is_week_ago { "%a " } else { "" }))
            .to_string()
    }
}

pub fn format_message_short_date(date: SystemTime, timezone: Tz) -> String {
    let date = DateTime::<Utc>::from(date).with_timezone(&timezone);
    date.format("%H:%M").to_string()
}

/// Fits `(w, h)` into `(max_w, max_h)` preserving aspect ratio, then grows the result back up
/// (still preserving aspect ratio) if needed so the width is at least `min_w` - e.g. so an
/// overlaid label has room to fit. If `max_h` doesn't allow reaching `min_w` without exceeding
/// it, `max_h` wins and the result stays narrower than `min_w`.
///
/// `min_h` is enforced as a hard floor instead: it's raised directly rather than by scaling
/// both dimensions together, since it exists purely to keep a label overlay readable, not to
/// preserve aspect ratio -- scaling both together would be a no-op whenever width is already
/// pinned at `max_w` (the common case for wide content).
pub fn fit_dimensions(
    w: f32,
    h: f32,
    max_w: f32,
    max_h: f32,
    min_w: f32,
    min_h: f32,
) -> (f32, f32) {
    if w == 0.0 || h == 0.0 {
        return (max_w, max_h);
    }

    let scale = (max_w / w).min(max_h / h).min(1.0);
    let (mut width, mut height) = (w * scale, h * scale);

    if width < min_w {
        // Capped by max_w/width too, not just max_h/height -- otherwise
        // growing to satisfy min_w can push width past max_w, and the
        // final clamp would then shrink width back down without rescaling
        // height, distorting the aspect ratio.
        let grow = (min_w / width).min(max_w / width).min(max_h / height);
        width *= grow;
        height *= grow;
    }

    height = height.max(min_h).min(max_h);

    (width.min(max_w), height)
}
