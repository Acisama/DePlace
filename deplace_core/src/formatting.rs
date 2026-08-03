use serde::{Deserialize, Serialize};

#[derive(Clone, PartialEq, Deserialize, Serialize)]
pub enum DataSizeUnit {
    Bytes,
    Bits,
    MibiBytes,
}

pub fn format_bytes(bytes: u64, unit: DataSizeUnit) -> String {
    let (size, units): (f64, [&str; 5]) = match unit {
        DataSizeUnit::Bytes => (bytes as f64, ["B", "KB", "MB", "GB", "TB"]),
        DataSizeUnit::MibiBytes => (bytes as f64, ["B", "KiB", "MiB", "GiB", "TiB"]),
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
