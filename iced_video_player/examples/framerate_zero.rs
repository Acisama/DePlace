// Reproduction of the "invalid framerate: 0" failure: a video whose H.264
// stream negotiates framerate 0/1 (variable/unknown) must open successfully.
fn main() {
    let path = std::env::args().nth(1).expect("usage: framerate_zero <file>");
    let url = url::Url::from_file_path(path).expect("file path");
    match iced_video_player::Video::new(&url) {
        Ok(video) => println!(
            "OK: opened, size {:?}, framerate {:.3}, duration {:?}",
            video.size(),
            video.framerate(),
            video.duration()
        ),
        Err(e) => {
            println!("FAILED: {e}");
            std::process::exit(1);
        }
    }
}
