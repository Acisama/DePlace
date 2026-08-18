use deplace_core::helpers::MatrixClientExt;
use gpui::App;
use matrix_sdk::{Client, ruma::events::room::MediaSource};

pub fn save_file(cx: &mut App, client: Client, source: &MediaSource, filename: &str) {
    tracing::trace!("Saving file: {}", filename);
    let Some(download_dir) = dirs::download_dir() else {
        return;
    };

    let path_rx = cx.prompt_for_new_path(&download_dir, Some(filename));
    let source = source.clone();
    let client = client.clone();

    cx.spawn(async move |_| {
        let path = match path_rx.await {
            Ok(Ok(Some(path))) => path,
            Ok(Err(e)) => {
                tracing::error!("Failed to prompt for new path: {:?}", e);
                return;
            }
            _ => {
                return;
            }
        };

        let bytes = match client.media().get_file_content(source, true).await {
            Ok(bytes) => bytes,
            Err(e) => {
                tracing::error!("Failed to get file content: {:?}", e);
                return;
            }
        };

        if let Err(e) = tokio::fs::write(path, bytes).await {
            tracing::error!("Failed to write file: {:?}", e);
        }
    })
    .detach();
}
