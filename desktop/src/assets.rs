use std::sync::Arc;

use gpui::{AssetSource, RenderImage};

#[derive(rust_embed::Embed)]
#[folder = "../assets/"]
pub struct AppAssets;

/// Decodes an embedded image synchronously, so it can be handed to `img()` as
/// already-rendered data instead of going through gpui's async asset cache
/// (which paints nothing, i.e. a flash of the window background, until the
/// first load completes).
pub fn decode_embedded_image(path: &str) -> anyhow::Result<Arc<RenderImage>> {
    let bytes = AppAssets::get(path)
        .ok_or_else(|| anyhow::anyhow!("Asset not found: {}", path))?
        .data;
    let mut rgba = image::load_from_memory(&bytes)?.into_rgba8();
    for pixel in rgba.chunks_exact_mut(4) {
        pixel.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

impl AssetSource for AppAssets {
    fn list(&self, path: &str) -> gpui::Result<Vec<gpui::SharedString>> {
        let mut assets: Vec<gpui::SharedString> = Self::iter()
            .filter_map(|p| p.starts_with(path).then_some(p.into()))
            .collect();
        assets.extend(gpui_component_assets::Assets.list(path)?);
        Ok(assets)
    }

    fn load(&self, path: &str) -> gpui::Result<Option<std::borrow::Cow<'static, [u8]>>> {
        if let Some(data) = Self::get(path) {
            return Ok(Some(data.data));
        }
        gpui_component_assets::Assets.load(path)
    }
}
