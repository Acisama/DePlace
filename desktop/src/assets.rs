use gpui::AssetSource;

#[derive(rust_embed::Embed)]
#[folder = "../assets/"]
pub struct AppAssets;

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
