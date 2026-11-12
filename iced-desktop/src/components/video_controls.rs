//! A self-contained video player: owns the [`DePlaceVideoShared`] cell
//! itself, so it can self-poll the *whole* lifecycle (load, play/pause, seek).
//!
//! The bar's own widgets talk in a private [`Local`] message type, handle what it produces right here
//! (toggling pause, seeking)

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use deplace_core::cache::MediaState;
use deplace_core::cache::video::{DePlaceVideoShared, LoadedVideo};
use deplace_core::formatting::format_bytes;
use deplace_core::state::media_fetcher::{DownloadProgress, MediaProcessor};
use deplace_core::structure::Structure;
use deplace_core::theme::Theme;
use iced::Length::Fill;
use iced::advanced::shell::Bus;
use iced::advanced::widget::{Meta, Operation, Tree, tree};
use iced::advanced::{Layout, Renderer as _, Shell, Widget, layout, mouse, overlay, renderer};
use iced::border::{self, Radius};
use iced::widget::Space;
use iced::{Alignment, Border, widget as w};
use iced::{
    Color, Element, Event, Length, Rectangle, Renderer, Size, Theme as IcedTheme, Vector,
    Widget as _, window,
};
use iced_video_player::{Video, VideoPlayer};

use crate::common::{pie_progress, structure, theme};
use crate::modules::home::DOWNLOAD_ICON;
use crate::{EMPTY_WIDGET, MONO_FONT};

use super::render_media::Fallback;
use super::{ColoredRounded, StatusExt, default_button, phosphor_icon};

/// How long the bar takes to slide fully in or out.
const SLIDE_DURATION: Duration = Duration::from_millis(200);
/// How long the real video takes to crossfade in over `fallback` once the
/// cell resolves to `Loaded` -- mirrors `RenderMedia`'s own fade duration.
const FADE_DURATION: Duration = Duration::from_millis(250);

enum VideoStatus {
    Downloading { text: String, percentage: f32 },
    Decrypting,
    Preparing,
    Failed,
}

impl VideoStatus {
    fn view(&self, theme: Theme, structure: Structure, opacity: f32) -> Element<'static, Local> {
        match self {
            VideoStatus::Downloading { text, percentage } => w::column![
                pie_progress(
                    *percentage,
                    structure.large_font_size * 2.0,
                    theme.text.normal.to_iced().scale_alpha(opacity),
                    structure.border_thickness,
                    None
                ),
                w::text(text.clone())
                    .size(structure.font_size)
                    .font(MONO_FONT)
                    .color(theme.text.normal.to_iced().scale_alpha(opacity))
            ]
            .padding(structure.small_gap)
            .spacing(structure.small_gap)
            .align_x(Alignment::Center)
            .boxed(),
            VideoStatus::Decrypting => w::column![
                phosphor_icon(
                    phosphor_svgs::icon::key::FILL,
                    structure.large_font_size * 2.0
                )
                .color(theme.text.normal.to_iced().scale_alpha(opacity)),
                w::text("Decrypting...".to_string())
                    .size(structure.font_size)
                    .font(MONO_FONT)
                    .color(theme.text.normal.to_iced().scale_alpha(opacity))
            ]
            .padding(structure.small_gap)
            .spacing(structure.small_gap)
            .align_x(Alignment::Center)
            .boxed(),
            VideoStatus::Preparing => w::column![
                phosphor_icon(
                    phosphor_svgs::icon::speedometer::FILL,
                    structure.large_font_size * 2.0
                )
                .color(theme.text.normal.to_iced().scale_alpha(opacity)),
                w::text("Preparing...".to_string())
                    .size(structure.font_size)
                    .font(MONO_FONT)
                    .color(theme.text.normal.to_iced().scale_alpha(opacity))
            ]
            .padding(structure.small_gap)
            .spacing(structure.small_gap)
            .align_x(Alignment::Center)
            .boxed(),
            VideoStatus::Failed => w::column![
                phosphor_icon(
                    phosphor_svgs::icon::warning::FILL,
                    structure.large_font_size * 2.0
                )
                .color(theme.colors.error.to_iced().scale_alpha(opacity)),
                w::text("Failed...".to_string())
                    .size(structure.font_size)
                    .font(MONO_FONT)
                    .color(theme.colors.error.to_iced().scale_alpha(opacity))
            ]
            .padding(structure.small_gap)
            .spacing(structure.small_gap)
            .align_x(Alignment::Center)
            .boxed(),
        }
    }
}

pub fn video_controls<'a, Message>(
    cell: DePlaceVideoShared,
    fallback: impl Fn(Radius, f32) -> Element<'a, Message> + 'a,
    media_processor: MediaProcessor,
    parameters_key: String,
    is_encrypted: bool,
    info_total: Option<u64>,
    is_loaded: bool,
) -> VideoControls<'a, Message>
where
    Message: 'a,
{
    VideoControls {
        cell,
        fallback: Box::new(fallback),
        radius: Radius::default(),
        content: None,
        not_started: None,
        bar: None,
        status_overlay: None,
        parameters_key,
        media_processor,
        is_encrypted,
        info_total,
        is_loaded,
    }
}

pub struct VideoControls<'a, Message> {
    cell: DePlaceVideoShared,
    fallback: Fallback<'a, Message>,
    radius: Radius,

    /// The fallback, crossfade, or real video player (built lazily)
    /// for whatever `Phase` the cell is currently in
    content: Option<Element<'a, Message>>,

    not_started: Option<Element<'a, Local>>,
    bar: Option<Element<'a, Local>>,
    status_overlay: Option<Element<'a, Local>>,

    parameters_key: String,
    media_processor: MediaProcessor,
    is_encrypted: bool,

    info_total: Option<u64>,
    is_loaded: bool,
}

/// Where the cell is in its lifecycle, mirroring
/// `RenderMedia`'s own `Phase` -- drives both what `content_for` draws and
/// whether `update` still needs to keep polling/animating every frame.
#[derive(Clone, Copy)]
enum Phase {
    NotStarted,
    Pending,
    Fading(Instant),
    Done,
}

#[derive(Clone, Copy, Debug)]
enum Local {
    TogglePause,
    Seek(f32),
    SeekReleased,
    StartDownload,
    ToggleMute,
    SetVolume(f32),
}

fn build_not_started(phase: Phase, theme: Theme, structure: Structure) -> Element<'static, Local> {
    match phase {
        Phase::NotStarted => w::container(
            w::button(
                w::row![
                    phosphor_icon(DOWNLOAD_ICON, structure.font_size * 1.1),
                    w::text("Download video").size(structure.font_size),
                ]
                .align_y(Alignment::Center)
                .spacing(structure.small_gap),
            )
            .style(move |_, status| w::button::Style {
                background: if status.active() {
                    Some(theme.solid_hover_bg.into())
                } else {
                    Some(theme.solid_bg.into())
                },
                text_color: if status.active() {
                    theme.text.normal.into()
                } else {
                    theme.text.dim.into()
                },
                border: Border {
                    color: theme.border.into(),
                    width: structure.border_thickness,
                    radius: structure.inner_border_radius.into(),
                },
                ..Default::default()
            })
            .on_press(Local::StartDownload),
        )
        .width(Fill)
        .height(Fill)
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .boxed(),
        _ => EMPTY_WIDGET.boxed(),
    }
}

fn build_bar(
    radius: Radius,
    paused: bool,
    progress: f32,
    muted: bool,
    volume: f32,
) -> Element<'static, Local> {
    let theme = theme();
    let structure = structure();

    let play_icon = if paused {
        phosphor_svgs::icon::play::FILL
    } else {
        phosphor_svgs::icon::pause::FILL
    };
    let volume_icon = if muted || volume <= 0.0 {
        phosphor_svgs::icon::speaker_slash::FILL
    } else {
        phosphor_svgs::icon::speaker_high::FILL
    };

    let content = w::column![
        w::row![
            default_button(
                phosphor_icon(play_icon, structure.large_font_size).color(theme.text.normal)
            )
            .on_press(Local::TogglePause),
            Space::new().width(Fill),
            default_button(
                phosphor_icon(volume_icon, structure.large_font_size).color(theme.text.normal)
            )
            .on_press(Local::ToggleMute),
            w::slider(0.0..=1.0, volume, Local::SetVolume)
                .step(0.01)
                .width(80.0)
                .style(move |_, _| w::slider::Style {
                    rail: w::slider::Rail {
                        backgrounds: (theme.accent.into(), theme.solid_hover_bg.into()),
                        width: structure.font_size / 4.0,
                        border: border::rounded(structure.font_size / 8.0),
                    },
                    handle: w::slider::Handle {
                        shape: w::slider::HandleShape::Circle {
                            radius: structure.font_size / 4.0
                        },
                        background: theme.accent.into(),
                        border_color: Color::TRANSPARENT,
                        border_width: 0.0,
                    },
                }),
        ]
        .spacing(structure.gap)
        .align_y(Alignment::Center),
        w::slider(0.0..=1.0, progress, Local::Seek)
            .step(0.001)
            .width(Fill)
            .on_release(Local::SeekReleased)
            .style(move |_, _| w::slider::Style {
                rail: w::slider::Rail {
                    backgrounds: (theme.accent.into(), theme.solid_hover_bg.into()),
                    width: structure.font_size / 2.0,
                    border: border::rounded(structure.font_size / 4.0),
                },
                handle: w::slider::Handle {
                    shape: w::slider::HandleShape::Circle {
                        radius: structure.font_size / 2.0
                    },
                    background: theme.accent.into(),
                    border_color: Color::TRANSPARENT,
                    border_width: 0.0,
                },
            }),
    ]
    .spacing(structure.small_gap);

    w::column![
        w::mouse_area(w::container(EMPTY_WIDGET).width(Fill).height(Fill))
            .on_press(Local::TogglePause),
        w::container(content)
            .width(Fill)
            .colored_rounded(
                theme.backdrop,
                Radius::default().bottom(radius.bottom_right),
            )
            .padding(structure.small_gap)
            .align_x(Alignment::Center)
            .align_y(Alignment::Center)
    ]
    .boxed()
}

fn build_status_overlay(status: Option<&VideoStatus>, alpha: f32) -> Element<'static, Local> {
    let theme = theme();
    let structure = structure();

    match status {
        Some(status) => w::container(status.view(theme, structure, alpha))
            .colored_rounded_with_border(
                theme.solid_bg.to_iced().scale_alpha(alpha),
                structure.inner_border_radius,
                structure.border_thickness,
                theme.border.to_iced().scale_alpha(alpha),
            )
            .boxed(),
        None => EMPTY_WIDGET.boxed(),
    }
}

/// How visible the status overlay should be for the given `Phase`: fully
/// shown throughout `Pending` (nothing else is fading yet), fading out in
/// step with the real video's own crossfade during `Fading`, then gone for
/// good once `Done`.
fn status_alpha(phase: Phase) -> f32 {
    match phase {
        Phase::Pending => 1.0,
        Phase::Fading(started) => {
            1.0 - (started.elapsed().as_secs_f32() / FADE_DURATION.as_secs_f32()).clamp(0.0, 1.0)
        }
        Phase::Done => 0.0,
        Phase::NotStarted => 0.0,
    }
}

fn video_element<'a, Message: Clone + 'a>(
    video: Arc<Video>,
    radius: Radius,
    opacity: f32,
) -> Element<'a, Message> {
    w::container(VideoPlayer::new(video).content_fit(iced::ContentFit::Cover))
        .width(Fill)
        .height(Fill)
        .clip(true)
        .style(move |_| {
            w::container::Style::default()
                .border(border::rounded(radius))
                .background(Color::WHITE.scale_alpha(opacity))
        })
        .boxed()
}

impl<'a, Message> VideoControls<'a, Message>
where
    Message: Clone + 'a,
{
    pub fn radius(mut self, radius: impl Into<Radius>) -> Self {
        self.radius = radius.into();
        self
    }

    fn video(&self) -> Option<Arc<Video>> {
        match self.cell.read().as_ref() {
            MediaState::Loaded((video, _feeder)) => Some(video.clone()),
            _ => None,
        }
    }

    fn content_for(&self, phase: Phase) -> Element<'a, Message> {
        match phase {
            Phase::Pending | Phase::NotStarted => (self.fallback)(self.radius, 1.0),
            Phase::Fading(started) => {
                let t =
                    (started.elapsed().as_secs_f32() / FADE_DURATION.as_secs_f32()).clamp(0.0, 1.0);

                match self.video() {
                    Some(video) => w::stack![
                        video_element(video, self.radius, t),
                        (self.fallback)(self.radius, 1.0 - t),
                    ]
                    .width(Fill)
                    .height(Fill)
                    .boxed(),
                    // Shouldn't happen (nothing moves the cell backwards
                    // out of `Loaded`), but fall back to the plain
                    // fallback rather than panicking if it ever does.
                    None => (self.fallback)(self.radius, 1.0),
                }
            }
            Phase::Done => match self.video() {
                Some(video) => video_element(video, self.radius, 1.0),
                None => (self.fallback)(self.radius, 1.0),
            },
        }
    }

    fn ensure_content(&mut self, phase: Phase) -> &mut Element<'a, Message> {
        if self.content.is_none() {
            self.content = Some(self.content_for(phase));
        }
        #[allow(clippy::unwrap_used)]
        self.content.as_mut().unwrap()
    }

    fn rebuild_content(&mut self, tree: &mut Tree, renderer: &Renderer, size: Size, phase: Phase) {
        let mut content = self.content_for(phase);
        tree.children[0].diff(&mut content);
        content.layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        self.content = Some(content);
    }

    fn ensure_not_started(&mut self, phase: Phase) -> &mut Element<'a, Local> {
        if self.not_started.is_none() {
            self.not_started = Some(build_not_started(phase, theme(), structure()));
        }
        #[allow(clippy::unwrap_used)]
        self.not_started.as_mut().unwrap()
    }

    fn rebuild_not_started(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        size: Size,
        phase: Phase,
    ) {
        let mut not_started = build_not_started(phase, theme(), structure());
        tree.children[3].diff(&mut not_started);
        not_started.layout(
            &mut tree.children[3],
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        tree.children[3].translation = Vector::ZERO;
        self.not_started = Some(not_started);
    }

    fn bar_for(&self, video: Option<&Arc<Video>>) -> Element<'static, Local> {
        match video {
            Some(video) => build_bar(
                self.radius,
                video.paused(),
                self.progress(None),
                // `Video::muted`/`set_muted` and `volume`/`set_volume`
                // read/write GStreamer's "mute"/"volume" properties
                // directly on the pipeline, which panics here: this app's
                // video pipelines are hand-built in
                // `video_stream::open_stream` (not a `playbin`), so there's
                // no such properties on them at all. Both hardcoded until
                // that's addressed -- see the matching notes by
                // `Local::ToggleMute`/`Local::SetVolume`.
                false,
                1.0,
            ),
            None => EMPTY_WIDGET.boxed(),
        }
    }

    fn ensure_bar(&mut self) -> &mut Element<'a, Local> {
        if self.bar.is_none() {
            let video = self.video();
            self.bar = Some(self.bar_for(video.as_ref()));
        }
        #[allow(clippy::unwrap_used)]
        self.bar.as_mut().unwrap()
    }

    #[allow(clippy::too_many_arguments)]
    fn rebuild_bar(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        width: f32,
        max_height: f32,
        paused: bool,
        progress: f32,
        muted: bool,
        volume: f32,
    ) -> f32 {
        let mut bar = build_bar(self.radius, paused, progress, muted, volume);
        tree.children[1].diff(&mut bar);
        bar.layout(
            &mut tree.children[1],
            renderer,
            &layout::Limits::new(Size::new(width, 0.0), Size::new(width, max_height)),
        );
        self.bar = Some(bar);
        tree.children[1].size.height
    }

    fn rebuild_bar_empty(&mut self, tree: &mut Tree, renderer: &Renderer, width: f32) -> f32 {
        let mut bar: Element<'a, Local> = EMPTY_WIDGET.boxed();
        tree.children[1].diff(&mut bar);
        bar.layout(
            &mut tree.children[1],
            renderer,
            &layout::Limits::new(Size::ZERO, Size::new(width, 0.0)),
        );
        self.bar = Some(bar);
        tree.children[1].size.height
    }

    fn ensure_status_overlay(&mut self, phase: Phase) -> &mut Element<'a, Local> {
        if self.status_overlay.is_none() {
            let status = self.download_status();
            self.status_overlay = Some(build_status_overlay(status.as_ref(), status_alpha(phase)));
        }
        #[allow(clippy::unwrap_used)]
        self.status_overlay.as_mut().unwrap()
    }

    fn rebuild_status_overlay(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        bounds: Size,
        status: Option<&VideoStatus>,
        phase: Phase,
    ) {
        let mut overlay = build_status_overlay(status, status_alpha(phase));
        tree.children[2].diff(&mut overlay);
        overlay.layout(
            &mut tree.children[2],
            renderer,
            &layout::Limits::new(Size::ZERO, bounds),
        );
        let size = tree.children[2].size;
        tree.children[2].translation = Vector::new(
            (bounds.width - size.width) / 2.0,
            (bounds.height - size.height) / 2.0,
        );
        self.status_overlay = Some(overlay);
    }

    /// Cheap enough to call every frame: `file_download_progress` is a
    /// `Mutex<IndexMap>` lookup that clones a couple of `Arc<AtomicU64>`s,
    /// the same cost class as `Video::position()`/`paused()`.
    fn download_status(&self) -> Option<VideoStatus> {
        match self
            .media_processor
            .file_download_progress(&self.parameters_key)?
        {
            DownloadProgress::Downloading {
                total_bytes,
                downloaded_bytes,
                ..
            } => {
                let downloaded = downloaded_bytes.load(Ordering::Relaxed);
                let total = total_bytes.load(Ordering::Relaxed);

                if total > 0 && downloaded >= total && self.is_encrypted {
                    return Some(VideoStatus::Decrypting);
                }

                let total = match total {
                    0 => self.info_total,
                    total => Some(total),
                };

                match total {
                    Some(total) => Some(VideoStatus::Downloading {
                        text: format!(
                            "Downloading Video ({}/{})",
                            format_bytes(downloaded, deplace_core::settings::DataSizeUnit::Bytes),
                            format_bytes(total, deplace_core::settings::DataSizeUnit::Bytes)
                        ),
                        percentage: downloaded as f32 / total as f32,
                    }),
                    None => Some(VideoStatus::Downloading {
                        text: format!(
                            "Downloading Video... ({})",
                            format_bytes(downloaded, deplace_core::settings::DataSizeUnit::Bytes)
                        ),
                        percentage: 0.0,
                    }),
                }
            }
            DownloadProgress::Completed { .. } => Some(VideoStatus::Preparing),
            DownloadProgress::Failed { .. } => Some(VideoStatus::Failed),
        }
    }

    fn progress(&self, dragging: Option<f32>) -> f32 {
        if let Some(fraction) = dragging {
            return fraction;
        }

        let Some(video) = self.video() else {
            return 0.0;
        };

        let duration = video.duration().as_secs_f64();
        if duration <= 0.0 {
            return 0.0;
        }

        (video.position().as_secs_f64() / duration).clamp(0.0, 1.0) as f32
    }
}

struct State {
    phase: Phase,
    shown: bool,
    transition: Option<(bool, Instant)>,
    dragging: Option<f32>,
    resume_after_drag: bool,
    last_seeked: Option<f32>,
}

impl State {
    fn new(phase: Phase) -> Self {
        State {
            phase,
            shown: false,
            transition: None,
            dragging: None,
            resume_after_drag: false,
            last_seeked: None,
        }
    }

    /// How far through the slide the bar currently is: `0.0` fully hidden
    /// below the bottom edge, `1.0` fully shown.
    fn amount(&self) -> f32 {
        let target = if self.shown { 1.0 } else { 0.0 };

        match self.transition {
            Some((from, started)) if started.elapsed() < SLIDE_DURATION => {
                let t = smoothstep(
                    (started.elapsed().as_secs_f32() / SLIDE_DURATION.as_secs_f32())
                        .clamp(0.0, 1.0),
                );
                let from_amount: f32 = if from { 1.0 } else { 0.0 };
                from_amount + (target - from_amount) * t
            }
            _ => target,
        }
    }
}

fn smoothstep(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

impl<'a, Message> Meta for VideoControls<'a, Message> {}

impl<'a, Message> Widget<Message, IcedTheme, Renderer> for VideoControls<'a, Message>
where
    Message: Clone + 'a,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        // A cache hit (already `Loaded` the moment this widget is built)
        // skips the fade entirely rather than animating in every time the
        // same row is scrolled back into view.
        let phase = match self.cell.read().as_ref() {
            MediaState::Loaded(_) => Phase::Done,
            MediaState::LoadNotStarted { .. } => {
                if self.is_loaded {
                    Phase::Pending
                } else {
                    Phase::NotStarted
                }
            }
            // `Loading`/`Failed`: the fetch is already underway (or already
            // failed) elsewhere
            _ => Phase::Pending,
        };
        tree::State::new(State::new(phase))
    }

    fn diff(&mut self, tree: &mut Tree) {
        let phase = tree.state.downcast_ref::<State>().phase;

        if tree.children.len() < 4 {
            tree.children.resize_with(4, Tree::empty);
        }
        tree.children[0].diff(self.ensure_content(phase));
        tree.children[1].diff(self.ensure_bar());
        tree.children[2].diff(self.ensure_status_overlay(phase));
        tree.children[3].diff(self.ensure_not_started(phase));
    }

    fn size(&self) -> Size<Length> {
        Size {
            width: Fill,
            height: Fill,
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let phase = tree.state.downcast_ref::<State>().phase;

        self.ensure_content(phase)
            .layout(&mut tree.children[0], renderer, limits);
        tree.children[0].translation = Vector::ZERO;

        let size = tree.children[0].size;
        tree.size = size;

        self.ensure_bar().layout(
            &mut tree.children[1],
            renderer,
            &layout::Limits::new(
                Size::new(size.width, 0.0),
                Size::new(size.width, size.height),
            ),
        );
        let bar_height = tree.children[1].size.height;

        let amount = if self.video().is_some() {
            tree.state.downcast_ref::<State>().amount()
        } else {
            0.0
        };
        tree.children[1].translation = Vector::new(0.0, size.height - bar_height * amount);

        self.ensure_status_overlay(phase).layout(
            &mut tree.children[2],
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        let overlay_size = tree.children[2].size;
        tree.children[2].translation = Vector::new(
            (size.width - overlay_size.width) / 2.0,
            (size.height - overlay_size.height) / 2.0,
        );

        self.ensure_not_started(phase).layout(
            &mut tree.children[3],
            renderer,
            &layout::Limits::new(Size::ZERO, size),
        );
        tree.children[3].translation = Vector::ZERO;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        operation.container(None, layout.bounds(), viewport);
        let phase = tree.state.downcast_ref::<State>().phase;

        let mut children = layout.iter_mut(&mut tree.children);
        if let Some((child_layout, child_tree)) = children.next() {
            self.ensure_content(phase).operate(
                child_tree,
                child_layout,
                viewport,
                renderer,
                operation,
            );
        }
        if let Some((child_layout, child_tree)) = children.next() {
            self.ensure_bar()
                .operate(child_tree, child_layout, viewport, renderer, operation);
        }
        let _ = children.next(); // Skip status overlay (index 2)
        if let Some((child_layout, child_tree)) = children.next() {
            self.ensure_not_started(phase).operate(
                child_tree,
                child_layout,
                viewport,
                renderer,
                operation,
            );
        }
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &Rectangle,
    ) {
        let bounds = layout.bounds();

        {
            let phase = tree.state.downcast_ref::<State>().phase;
            let mut children = layout.iter_mut(&mut tree.children);
            if let Some((content_layout, content_tree)) = children.next() {
                self.ensure_content(phase).update(
                    content_tree,
                    event,
                    content_layout,
                    cursor,
                    renderer,
                    shell,
                    viewport,
                );
            }
        }

        if shell.is_event_captured() {
            return;
        }

        if let Event::Window(window::Event::RedrawRequested(_)) = event {
            let status = self.download_status();
            let video = self.video();
            let size = Size::new(bounds.width, bounds.height);

            // Phase transitions.
            {
                let state = tree.state.downcast_mut::<State>();

                if self.is_loaded
                    && matches!(self.cell.read().as_ref(), MediaState::LoadNotStarted { .. })
                {
                    self.cell.clone().start_loading::<LoadedVideo>(true);
                }

                match state.phase {
                    Phase::NotStarted
                        if !matches!(
                            self.cell.read().as_ref(),
                            MediaState::LoadNotStarted { .. }
                        ) =>
                    {
                        state.phase = Phase::Pending;
                    }
                    // Always fades, even for an already-on-disk video `is_loaded`
                    // auto-starts: that only skips the "Download video" button
                    // (and the `NotStarted` phase it belongs to) -- the load
                    // itself still takes a moment once this widget is built, so
                    // it still deserves the same crossfade a slower download
                    // gets once it resolves. `state()`'s own cache-hit check is
                    // what skips the fade for a cell that's *already* `Loaded`
                    // by construction time.
                    Phase::Pending if video.is_some() => {
                        state.phase = Phase::Fading(Instant::now());
                    }
                    Phase::Fading(started) if started.elapsed() >= FADE_DURATION => {
                        state.phase = Phase::Done;
                    }
                    _ => {}
                }
            }
            let phase = tree.state.downcast_ref::<State>().phase;

            // `download_status()` reflects `media_processor`'s own tracked
            // progress, which deliberately keeps a `Completed`/`Failed` entry
            // around forever once set (the header's own download indicator
            // relies on that to keep showing the last outcome) -- so it never
            // naturally goes back to `None` on its own. Once this video's own
            // `Phase` is `Done` there's nothing left for *this* overlay to
            // say regardless of what that still reports, so it's forced to
            // `None` here rather than leaning on `status_alpha(Done) == 0.0`
            // alone to hide it: this also stops `status.is_some()` below from
            // keeping the redraw loop spinning forever once settled.
            let status = if matches!(phase, Phase::Done) {
                None
            } else {
                status
            };

            self.rebuild_content(tree, renderer, size, phase);
            self.rebuild_not_started(tree, renderer, size, phase);
            self.rebuild_status_overlay(tree, renderer, size, status.as_ref(), phase);

            let mut bar_animating = false;

            if let Some(video) = &video
                && !matches!(phase, Phase::Pending)
            {
                let (amount, transitioning, dragging_active, paused, progress) = {
                    let state = tree.state.downcast_mut::<State>();

                    let hovered = cursor.is_over(bounds);
                    let target_shown = hovered || video.paused();
                    if target_shown != state.shown {
                        state.transition = Some((state.shown, Instant::now()));
                        state.shown = target_shown;
                    }

                    if let Some(value) = state.dragging
                        && state.last_seeked != Some(value)
                    {
                        state.last_seeked = Some(value);
                        // TODO: Make this performant enough so it can be done live
                        // let target = video.duration().mul_f32(value);
                        // if let Err(error) = video.seek(target, false) {
                        //     tracing::warn!(?error, "failed to seek video");
                        // }
                    }

                    (
                        state.amount(),
                        state
                            .transition
                            .is_some_and(|(_, started)| started.elapsed() < SLIDE_DURATION),
                        state.dragging.is_some(),
                        video.paused(),
                        self.progress(state.dragging),
                    )
                };

                let bar_height = self.rebuild_bar(
                    tree,
                    renderer,
                    bounds.width,
                    bounds.height,
                    paused,
                    progress,
                    false,
                    1.0,
                );
                tree.children[1].translation =
                    Vector::new(0.0, bounds.height - bar_height * amount);

                bar_animating = transitioning || (amount > 0.0 && !paused) || dragging_active;
            } else {
                self.rebuild_bar_empty(tree, renderer, bounds.width);
                tree.children[1].translation = Vector::new(0.0, bounds.height);
            }

            self.rebuild_status_overlay(tree, renderer, size, status.as_ref(), phase);

            if matches!(phase, Phase::Pending | Phase::Fading(_))
                || bar_animating
                || status.is_some()
            {
                shell.request_redraw();
            }

            return;
        }

        let phase = tree.state.downcast_ref::<State>().phase;

        let mut local_bus = Bus::new();
        let mut local_shell = shell.local(&mut local_bus);

        if matches!(phase, Phase::NotStarted)
            && let Some((ns_layout, ns_tree)) = layout.iter_mut(&mut tree.children).nth(3)
            && let Some(not_started) = &mut self.not_started
        {
            not_started.update(
                ns_tree,
                event,
                ns_layout,
                cursor,
                renderer,
                &mut local_shell,
                viewport,
            );
        }

        if let Some(video) = self.video() {
            let amount = tree.state.downcast_ref::<State>().amount();

            if amount >= 0.5 {
                let paused = video.paused();
                let progress = self.progress(tree.state.downcast_ref::<State>().dragging);

                self.rebuild_bar(
                    tree,
                    renderer,
                    bounds.width,
                    bounds.height,
                    paused,
                    progress,
                    false,
                    1.0,
                );

                if let Some((bar_layout, bar_tree)) = layout.iter_mut(&mut tree.children).nth(1)
                    && let Some(bar) = &mut self.bar
                {
                    bar.update(
                        bar_tree,
                        event,
                        bar_layout,
                        cursor,
                        renderer,
                        &mut local_shell,
                        viewport,
                    );
                }
            }
        }

        shell.invalidate(local_shell.invalidation());
        shell.request_redraw_at(local_shell.redraw_request());
        if local_shell.is_event_captured() {
            shell.capture_event();
        }

        if local_bus.is_empty() {
            return;
        }

        let state = tree.state.downcast_mut::<State>();
        for (message, _receipt) in local_bus.drain() {
            match message {
                Local::TogglePause => {
                    if let Some(video) = self.video() {
                        video.set_paused(!video.paused());
                    }
                }
                Local::Seek(value) => {
                    if let Some(video) = self.video() {
                        if state.dragging.is_none() {
                            state.resume_after_drag = !video.paused();
                            video.set_paused(true);
                        }
                        state.dragging = Some(value);
                    }
                }
                Local::SeekReleased => {
                    if let Some(video) = self.video()
                        && let Some(value) = state.dragging.take()
                    {
                        let target = video.duration().mul_f32(value);
                        if let Err(error) = video.seek(target, false) {
                            tracing::warn!(?error, "failed to seek video");
                        }
                        video.set_paused(!state.resume_after_drag);
                    }
                }
                Local::StartDownload => {
                    self.cell.clone().start_loading::<LoadedVideo>(false);
                    shell.request_redraw();
                }
                Local::ToggleMute => {
                    // Deliberately a no-op: `Video::set_muted` sets
                    // GStreamer's "mute" property directly on the
                    // pipeline, which panics on this app's hand-built
                    // pipelines (see the note on `muted` in `bar_for`) --
                    // not calling it until that's fixed properly.
                }
                Local::SetVolume(_value) => {
                    // Deliberately a no-op: `Video::set_volume` sets
                    // GStreamer's "volume" property directly on the
                    // pipeline (and internally calls `set_muted` too),
                    // which panics on this app's hand-built pipelines (see
                    // the note on `muted`/`volume` in `bar_for`) -- not
                    // calling it until that's fixed properly.
                }
            }
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        let amount = tree.state.downcast_ref::<State>().amount();

        if amount >= 0.5
            && let Some(bar) = &self.bar
            && let Some((bar_layout, bar_tree)) = layout.iter(&tree.children).nth(1)
        {
            let interaction =
                bar.mouse_interaction(bar_tree, bar_layout, cursor, viewport, renderer);
            if interaction != mouse::Interaction::default() {
                return interaction;
            }
        }

        let phase = tree.state.downcast_ref::<State>().phase;
        if matches!(phase, Phase::NotStarted)
            && let Some(ns) = &self.not_started
            && let Some((ns_layout, ns_tree)) = layout.iter(&tree.children).nth(3)
        {
            let interaction = ns.mouse_interaction(ns_tree, ns_layout, cursor, viewport, renderer);
            if interaction != mouse::Interaction::default() {
                return interaction;
            }
        }

        match (&self.content, layout.iter(&tree.children).next()) {
            (Some(content), Some((content_layout, content_tree))) => {
                content.mouse_interaction(content_tree, content_layout, cursor, viewport, renderer)
            }
            _ => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &IcedTheme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        if let Some(content) = &self.content
            && let Some((content_layout, content_tree)) = layout.iter(&tree.children).next()
        {
            content.draw(
                content_tree,
                renderer,
                theme,
                style,
                content_layout,
                cursor,
                viewport,
            );
        }

        let bounds = layout.bounds();
        let Some(clip) = bounds.intersection(viewport) else {
            return;
        };

        if let Some(overlay) = &self.status_overlay
            && let Some((overlay_layout, overlay_tree)) = layout.iter(&tree.children).nth(2)
        {
            renderer.with_layer(clip, |renderer| {
                overlay.draw(
                    overlay_tree,
                    renderer,
                    theme,
                    style,
                    overlay_layout,
                    cursor,
                    viewport,
                );
            });
        }

        let amount = tree.state.downcast_ref::<State>().amount();

        if amount > 0.0
            && let Some(bar) = &self.bar
            && let Some((bar_layout, bar_tree)) = layout.iter(&tree.children).nth(1)
        {
            renderer.with_layer(clip, |renderer| {
                bar.draw(
                    bar_tree, renderer, theme, style, bar_layout, cursor, viewport,
                );
            });
        }

        let phase = tree.state.downcast_ref::<State>().phase;
        if matches!(phase, Phase::NotStarted)
            && let Some(not_started) = &self.not_started
            && let Some((ns_layout, ns_tree)) = layout.iter(&tree.children).nth(3)
        {
            renderer.with_layer(clip, |renderer| {
                not_started.draw(ns_tree, renderer, theme, style, ns_layout, cursor, viewport);
            });
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, IcedTheme, Renderer>> {
        let phase = tree.state.downcast_ref::<State>().phase;
        let Some((content_layout, content_tree)) = layout.iter_mut(&mut tree.children).next()
        else {
            return vec![];
        };

        self.ensure_content(phase).overlay(
            content_tree,
            content_layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message> From<VideoControls<'a, Message>> for Element<'a, Message>
where
    Message: Clone + 'a,
{
    fn from(widget: VideoControls<'a, Message>) -> Self {
        widget.boxed()
    }
}
