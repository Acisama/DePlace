use std::process::exit;

fn main() {
    // Embeds deplace_icon.ico as a PE resource so the built .exe has an icon
    // of its own - without this, the icon only ever gets set at runtime via
    // iced's window::Settings (visible in the taskbar once running), leaving
    // Explorer, Start Menu search, and shortcuts with no icon to show.
    // No-op on non-Windows targets.
    if let Err(e) = embed_resource::compile("windows.rc", embed_resource::NONE).manifest_optional()
    {
        eprintln!("Failed to compile windows.rc: {}", e);
        exit(1)
    }
}
