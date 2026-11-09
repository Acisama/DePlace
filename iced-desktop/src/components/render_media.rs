//! A leaf widget for media backed by a [`Shared<MediaState<_>>`] cell.
//!
//! Draws the caller-supplied `success` content once the cell resolves to
//! [`MediaState::Loaded`]; otherwise draws `fallback(width, height, radius)`
//! for every other state -- `Loading`, `LoadNotStarted`, and `Failed` all
//! look the same here. Callers that want to show something different on
//! failure (an icon, a retry button, ...) should fold that into `fallback`
//! themselves.
//!
//! No `NeedsMedia`/`MediaLoaded` messages are involved: `start` is called
//! once, right here when the widget is built (safe to call more than once
//! if the row ever gets rebuilt -- `Shared::start_loading` already claims
//! the fetch atomically on the cache side, so a redundant call is a cheap
//! no-op). After that the cell is simply re-checked on every
//! `RedrawRequested` frame until it resolves -- the same self-polling
//! trick [`super::elapsed_time`] and [`super::animated_button`] already
//! use for their own per-frame state.

use std::sync::Arc;
use std::time::{Duration, Instant};

use deplace_core::{
    Shared,
    cache::{DePlaceImage, DePlaceThumbnailShared, MediaState, ThumbnailLoader},
};
use iced::Length::Fill;
use iced::{
    Color, Element, Event, Length, Renderer, Size, Theme, Vector, Widget as _,
    advanced::{
        Layout, Shell, Widget, layout, mouse, overlay, renderer,
        widget::{Meta, Operation, Tree, tree},
    },
    border::{self, Radius},
    widget::{self as w},
    window,
};

/// How long the fallback takes to fade out once the real content is ready.
const FADE_DURATION: Duration = Duration::from_millis(250);

/// `width`/`height` aren't passed to either closure: `RenderMedia` already
/// owns that sizing contract via its own `width`/`height` fields, which
/// `layout()` turns into the `Limits` whatever this content resolves to
/// gets laid out with -- regardless of whether that happens through an
/// outer wrapping `Container` or through `RenderMedia`'s own `size()`
/// being read by a plain `Row`/`Column`. Implementations just need to use
/// `Length::Fill` for their own root widget to pick that up.
type Success<'a, Message, I> = Box<dyn Fn(&I, Radius) -> Element<'a, Message> + 'a>;
/// `alpha` goes from `1.0` (freshly shown) down to `0.0` (fully faded out,
/// right before being dropped) while the real content fades in underneath.
pub type Fallback<'a, Message> = Box<dyn Fn(Radius, f32) -> Element<'a, Message> + 'a>;

/// Builds a [`RenderMedia`]. `start` is called once, right away, with a
/// clone of `cell` -- typically `|cell| cell.start_loading::<ImageLoader>()`.
/// `success` is called every time the cell is (re-)found to be
/// [`MediaState::Loaded`], to build the actual content; `fallback` is used
/// for every other state, and continues to be called with a decreasing
/// `alpha` while the real content fades in underneath it.
pub fn render_media<'a, Message, I>(
    cell: Arc<Shared<MediaState<I>>>,
    start: impl FnOnce(Arc<Shared<MediaState<I>>>) + 'a,
    width: impl Into<Length>,
    height: impl Into<Length>,
    radius: impl Into<Radius>,
    success: impl Fn(&I, Radius) -> Element<'a, Message> + 'a,
    fallback: impl Fn(Radius, f32) -> Element<'a, Message> + 'a,
) -> RenderMedia<'a, Message, I>
where
    Message: 'a,
    I: Clone + Sync + Send + 'static,
{
    RenderMedia::new(cell, start, width, height, radius, success, fallback)
}

pub struct RenderMedia<'a, Message, I: Clone> {
    cell: Arc<Shared<MediaState<I>>>,
    success: Success<'a, Message, I>,
    fallback: Fallback<'a, Message>,

    width: Length,
    height: Length,
    radius: Radius,

    /// Published once, the moment the cell resolves
    on_resolved: Option<Message>,

    /// Built lazily, the first time the widget is actually processed
    content: Option<Element<'a, Message>>,
}

/// Where the widget is in its lifecycle, driving both what gets drawn and
/// whether it still needs to keep polling/animating every frame.
#[derive(Clone, Copy)]
enum Phase {
    /// Still `LoadNotStarted`/`Loading`: showing the plain fallback,
    /// checking the cell every frame.
    Pending,
    /// Resolved to `Loaded`; fading the fallback out over the real content
    /// since this `Instant`.
    Fading(Instant),
    /// Fully settled -- either the fade finished, or the cell resolved to
    /// `Failed` (which has nothing to fade to, so it skips straight here).
    /// Nothing left to poll or animate.
    Done,
}

struct State {
    phase: Phase,
}

impl<'a, Message, I> RenderMedia<'a, Message, I>
where
    Message: 'a,
    I: Clone + Sync + Send + 'static,
{
    pub fn new(
        cell: Arc<Shared<MediaState<I>>>,
        start: impl FnOnce(Arc<Shared<MediaState<I>>>) + 'a,
        width: impl Into<Length>,
        height: impl Into<Length>,
        radius: impl Into<Radius>,
        success: impl Fn(&I, Radius) -> Element<'a, Message> + 'a,
        fallback: impl Fn(Radius, f32) -> Element<'a, Message> + 'a,
    ) -> Self {
        // Safe to call every time this gets (re)built, not just the first:
        // `start_loading` already no-ops once the cell has moved past
        // `LoadNotStarted`.
        start(cell.clone());

        Self {
            cell,
            success: Box::new(success),
            fallback: Box::new(fallback),
            width: width.into(),
            height: height.into(),
            radius: radius.into(),
            on_resolved: None,
            content: None,
        }
    }

    /// Sets a message to publish once, the moment the cell resolves. See
    /// the field doc on `on_resolved` for why this is needed at all.
    pub fn on_resolved(mut self, message: Message) -> Self {
        self.on_resolved = Some(message);
        self
    }

    /// Builds `content` if it hasn't been already. Called from `diff()`
    /// (and defensively from `layout()`/`operate()`/`overlay()` in case
    /// something ever calls one of those without a `diff()` first) -- by
    /// then `width`/`height`/`radius` reflect whatever the builder chain
    /// set, since that's all plain synchronous Rust that already ran
    /// before this widget is ever handed to iced.
    fn ensure_content(&mut self, phase: Phase) -> &mut Element<'a, Message> {
        if self.content.is_none() {
            self.content = Some(Self::content_for(
                &self.cell,
                &self.success,
                &self.fallback,
                self.width,
                self.height,
                self.radius,
                phase,
            ));
        }
        #[allow(clippy::unwrap_used)]
        self.content.as_mut().unwrap()
    }

    /// Rebuilds `content` unconditionally (unlike `ensure_content`) for the
    /// given `phase`, re-diffs it into `tree`, and lays it out immediately
    /// using `layout`'s own bounds as the limits -- done right here rather
    /// than waiting for the parent to call our `layout()` again, since a
    /// virtualized-list row isn't guaranteed to ever do that on its own
    /// (see the comment on `on_resolved`). Used for every phase transition
    /// and for each fade tick.
    fn rebuild_content(
        &mut self,
        tree: &mut Tree,
        renderer: &Renderer,
        layout: Layout,
        phase: Phase,
    ) {
        self.content = Some(Self::content_for(
            &self.cell,
            &self.success,
            &self.fallback,
            self.width,
            self.height,
            self.radius,
            phase,
        ));

        #[allow(clippy::unwrap_used)]
        let content = self.content.as_mut().unwrap();
        tree.diff_children(std::slice::from_mut(content));
        content.layout(
            &mut tree.children[0],
            renderer,
            &layout::Limits::new(Size::ZERO, layout.bounds().size()),
        );
    }

    fn content_for(
        cell: &Shared<MediaState<I>>,
        success: &Success<'a, Message, I>,
        fallback: &Fallback<'a, Message>,
        width: Length,
        height: Length,
        radius: Radius,
        phase: Phase,
    ) -> Element<'a, Message> {
        match phase {
            Phase::Pending => fallback(radius, 1.0),
            Phase::Done => match cell.read().as_ref() {
                MediaState::Loaded(item) => success(item, radius),
                _ => fallback(radius, 1.0),
            },
            Phase::Fading(start) => {
                let t = (start.elapsed().as_secs_f32() / FADE_DURATION.as_secs_f32()).min(1.0);
                match cell.read().as_ref() {
                    // The stack is the one thing here that still needs an
                    // explicit size: both layers are `Fill`-sized to cover
                    // whatever box *it* is given, so something has to be
                    // the authority on what that box actually is.
                    MediaState::Loaded(item) => {
                        w::stack![success(item, radius), fallback(radius, 1.0 - t),]
                            .width(width)
                            .height(height)
                            .boxed()
                    }
                    // Shouldn't happen (nothing moves a cell backwards out
                    // of `Loaded`), but fall back to the plain fallback
                    // rather than panicking if it ever does.
                    _ => fallback(radius, 1.0 - t),
                }
            }
        }
    }

    /// What `phase` should start as, based on the cell's state right now --
    /// e.g. a cache hit (already `Loaded`/`Failed` the moment this widget
    /// is built) skips the fade entirely rather than animating in every
    /// time the same row is scrolled back into view.
    fn initial_phase(&self) -> Phase {
        match self.cell.read().as_ref() {
            MediaState::Loaded(_) | MediaState::Failed(_) => Phase::Done,
            MediaState::LoadNotStarted { .. } | MediaState::Loading => Phase::Pending,
        }
    }

    /// Called once, the moment the widget is fully settled (no more
    /// polling or animating left). `invalidate_layout`/a swapped `content`
    /// aren't enough on their own inside a virtualized list row:
    /// `sweeten::List::layout` only re-lays-out rows that have a queued
    /// `Content::Change::Updated` entry, which nothing here produces by
    /// itself. Publishing a message that ends up calling
    /// `Content::get_mut`/`get_index_mut` for this row (even as a no-op)
    /// is what actually queues that entry, so the list does a real layout
    /// pass for the final content instead of drawing it into stale/blank
    /// layout data.
    fn on_settled(&mut self, shell: &mut Shell<'_, Message>) {
        if let Some(message) = self.on_resolved.take() {
            shell.publish(message);
        }
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn radius(mut self, radius: impl Into<Radius>) -> Self {
        self.radius = radius.into();
        self
    }

    pub fn success(mut self, success: Success<'a, Message, I>) -> Self {
        self.success = success;
        self
    }

    pub fn fallback(mut self, fallback: Fallback<'a, Message>) -> Self {
        self.fallback = fallback;
        self
    }

    pub fn maybe_fallback(mut self, fallback: Option<Fallback<'a, Message>>) -> Self {
        if let Some(fallback) = fallback {
            self.fallback = fallback;
        }
        self
    }
}

impl<'a, Message, I: Clone> Meta for RenderMedia<'a, Message, I> {}

impl<'a, Message, I> Widget<Message, Theme, Renderer> for RenderMedia<'a, Message, I>
where
    Message: 'a,
    I: Clone + Sync + Send + 'static,
{
    fn tag(&self) -> tree::Tag {
        tree::Tag::of::<State>()
    }

    fn state(&self) -> tree::State {
        tree::State::new(State {
            phase: self.initial_phase(),
        })
    }

    fn diff(&mut self, tree: &mut Tree) {
        let phase = tree.state.downcast_ref::<State>().phase;
        tree.diff_children(std::slice::from_mut(self.ensure_content(phase)));
    }

    fn size(&self) -> Size<Length> {
        match &self.content {
            Some(content) => content.size(),
            // Not built yet (diff() hasn't run): report what the box was
            // asked for directly, which is exactly what content_for would
            // end up sizing itself to anyway.
            None => Size {
                width: self.width,
                height: self.height,
            },
        }
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &Renderer, limits: &layout::Limits) {
        let phase = tree.state.downcast_ref::<State>().phase;
        self.ensure_content(phase)
            .layout(&mut tree.children[0], renderer, limits);
        // Our own reported size, read back by whoever laid *us* out (e.g.
        // `Container`) to build the `Layout`/bounds passed into `draw()`.
        // Left unset it defaults to `Size::ZERO`, so the parent ends up
        // handing `draw()` a zero-area rectangle no matter what the child
        // actually resolved to -- same reason `Lazy::layout()` (and
        // `AnimatedButton::layout()`) set this explicitly too.
        tree.size = tree.children[0].size;
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
        operation: &mut dyn Operation,
    ) {
        let phase = tree.state.downcast_ref::<State>().phase;
        self.ensure_content(phase).operate(
            &mut tree.children[0],
            layout,
            viewport,
            renderer,
            operation,
        );
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &Renderer,
        shell: &mut Shell<'_, Message>,
        viewport: &iced::Rectangle,
    ) {
        let phase = tree.state.downcast_ref::<State>().phase;

        self.ensure_content(phase).update(
            &mut tree.children[0],
            event,
            layout,
            cursor,
            renderer,
            shell,
            viewport,
        );

        if matches!(phase, Phase::Done) {
            return;
        }

        let Event::Window(window::Event::RedrawRequested(_)) = event else {
            return;
        };

        match phase {
            Phase::Pending => {
                // An owned `Arc` rather than `read()`'s guard: the branches
                // below need a full `&mut self`, which can't happen while a
                // guard borrowed from `self.cell` (holding `item: &I`) is
                // still alive for the match.
                let resolved = self.cell.get();

                match resolved.as_ref() {
                    MediaState::Loaded(_) => {
                        let start = Instant::now();
                        tree.state.downcast_mut::<State>().phase = Phase::Fading(start);
                        self.rebuild_content(tree, renderer, layout, Phase::Fading(start));
                        shell.invalidate_layout();
                        shell.request_redraw();
                    }
                    MediaState::Failed(_) => {
                        // Nothing to fade to; the fallback already showing
                        // is exactly what `Done` would draw for `Failed`.
                        tree.state.downcast_mut::<State>().phase = Phase::Done;
                        self.on_settled(shell);
                    }
                    MediaState::LoadNotStarted { .. } | MediaState::Loading => {
                        // Still in flight -- keep getting woken up every
                        // frame until it resolves.
                        shell.request_redraw();
                    }
                }
            }
            Phase::Fading(start) => {
                if start.elapsed() >= FADE_DURATION {
                    tree.state.downcast_mut::<State>().phase = Phase::Done;
                    self.rebuild_content(tree, renderer, layout, Phase::Done);
                    shell.invalidate_layout();
                    self.on_settled(shell);
                } else {
                    self.rebuild_content(tree, renderer, layout, phase);
                    shell.invalidate_layout();
                    shell.request_redraw();
                }
            }
            Phase::Done => {}
        }
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
        renderer: &Renderer,
    ) -> mouse::Interaction {
        match &self.content {
            Some(content) => {
                content.mouse_interaction(&tree.children[0], layout, cursor, viewport, renderer)
            }
            None => mouse::Interaction::default(),
        }
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut Renderer,
        theme: &Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &iced::Rectangle,
    ) {
        // Should always be `Some` by now (`diff()`/`layout()` build it
        // lazily and always run before `draw()`); nothing to draw yet if
        // something calls `draw()` without either having run first.
        if let Some(content) = &self.content {
            content.draw(
                &tree.children[0],
                renderer,
                theme,
                style,
                layout,
                cursor,
                viewport,
            );
        }
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &Renderer,
        viewport: &iced::Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, Message, Theme, Renderer>> {
        let phase = tree.state.downcast_ref::<State>().phase;
        self.ensure_content(phase).overlay(
            &mut tree.children[0],
            layout,
            renderer,
            viewport,
            translation,
            window,
        )
    }
}

impl<'a, Message, I> From<RenderMedia<'a, Message, I>> for Element<'a, Message>
where
    Message: 'a,
    I: Clone + Sync + Send + 'static,
{
    fn from(widget: RenderMedia<'a, Message, I>) -> Self {
        widget.boxed()
    }
}

pub fn render_thumbnail<'a, Message: 'a>(
    cell: DePlaceThumbnailShared,
) -> RenderMedia<'a, Message, DePlaceImage> {
    RenderMedia::new(
        cell,
        |cell| cell.start_loading::<ThumbnailLoader>(),
        Length::Shrink,
        Length::Shrink,
        0.0,
        render_deplace_image,
        render_image_fallback,
    )
}

fn render_deplace_image<'a, Message>(image: &DePlaceImage, radius: Radius) -> Element<'a, Message> {
    match image {
        DePlaceImage::Raster(handle) => w::image(handle)
            .width(Fill)
            .height(Fill)
            .border_radius(radius)
            .content_fit(iced::ContentFit::Cover)
            .boxed(),
        DePlaceImage::Svg(handle) => w::svg(handle.clone()).width(Fill).height(Fill).boxed(),
    }
}

fn render_image_fallback<'a, Message>(radius: Radius, opacity: f32) -> Element<'a, Message> {
    w::container(w::text("Image not available"))
        .width(Fill)
        .height(Fill)
        .style(move |_| {
            w::container::Style::default()
                .border(border::rounded(radius))
                .background(Color::WHITE.scale_alpha(opacity))
        })
        .boxed()
}
