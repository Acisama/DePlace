//! A `StyledText` with click actions and drag-to-select text that can span multiple sibling
//! elements (even across messages) as one continuous selection.
//!
//! `gpui::InteractiveText` gets us click ranges and a hover-index callback, but nothing about
//! text selection - there's no public API for it anywhere in `gpui`. Selecting and clicking
//! are also fundamentally the same mouse gesture (down, maybe drag, up) disambiguated only by
//! "did it move", so bolting a separate selection element on top of `InteractiveText` would
//! mean two elements fighting over the same mouse events. This implements both directly on
//! `gpui`'s own hit-testing primitives (`TextLayout::index_for_position`), the same ones
//! `InteractiveText` itself is built on.
//!
//! Cross-element selection state lives in `render::SelectionState` (backed by `ChatView`, an
//! `Entity`, not `gpui`'s per-element state) - see its docs for why. This element only needs
//! its own position (`message_index`, `element_index`) plus that shared state's current
//! (anchor, cursor) and three callbacks to decide its own highlight range and react to drags.

use std::{cell::Cell, ops::Range, rc::Rc};

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, Element, ElementId, GlobalElementId, Hitbox,
    HitboxBehavior, Hsla, InspectorElementId, IntoElement, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, Point, SharedString, StyledText, TextLayout, TextRun,
    Window, fill, point,
};

use crate::components::message::TextCoord;

/// What activating a run (a click with no drag) does.
#[derive(Clone)]
pub(crate) enum RunAction {
    OpenUrl(SharedString),
}

/// Per-element hover-change detection only (not selection - that's shared, see module docs).
/// Needed to avoid calling `on_hover`/notifying on every single mouse-move frame when the
/// hovered run hasn't actually changed.
#[derive(Clone, Default)]
struct HoverLocalState {
    hovered_range: Rc<Cell<Option<usize>>>,
}

pub(crate) struct SelectableRichText {
    id: ElementId,
    message_index: usize,
    element_index: usize,
    text: StyledText,
    full_text: SharedString,
    click_ranges: Vec<Range<usize>>,
    click_actions: Vec<RunAction>,
    hover_keys: Vec<SharedString>,
    on_hover: Option<Rc<dyn Fn(Option<SharedString>, &mut Window, &mut App)>>,
    selection_current: Option<(TextCoord, TextCoord)>,
    on_selection_start: Option<Rc<dyn Fn(TextCoord, &mut App)>>,
    on_selection_extend: Option<Rc<dyn Fn(TextCoord, &mut App)>>,
    on_selection_finish: Option<Rc<dyn Fn(&mut App)>>,
    selection_color: Hsla,
}

impl SelectableRichText {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: impl Into<ElementId>,
        message_index: usize,
        element_index: usize,
        text: SharedString,
        runs: Vec<TextRun>,
        click_ranges: Vec<Range<usize>>,
        click_actions: Vec<RunAction>,
        hover_keys: Vec<SharedString>,
        selection_color: Hsla,
    ) -> Self {
        Self {
            id: id.into(),
            message_index,
            element_index,
            text: StyledText::new(text.clone()).with_runs(runs),
            full_text: text,
            click_ranges,
            click_actions,
            hover_keys,
            on_hover: None,
            selection_current: None,
            on_selection_start: None,
            on_selection_extend: None,
            on_selection_finish: None,
            selection_color,
        }
    }

    /// Called when the hovered run changes, with the run's stable key (from `hover_keys`) or
    /// `None` when the mouse leaves every clickable run.
    pub(crate) fn on_hover(
        mut self,
        listener: impl Fn(Option<SharedString>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_hover = Some(Rc::new(listener));
        self
    }

    /// Wires this element into the shared cross-element/cross-message selection: `current` is
    /// this render's (anchor, cursor) snapshot (used to decide this element's own highlight
    /// range), and `start`/`extend`/`finish` mirror `render::SelectionState`'s callbacks.
    pub(crate) fn on_selection(
        mut self,
        current: Option<(TextCoord, TextCoord)>,
        start: impl Fn(TextCoord, &mut App) + 'static,
        extend: impl Fn(TextCoord, &mut App) + 'static,
        finish: impl Fn(&mut App) + 'static,
    ) -> Self {
        self.selection_current = current;
        self.on_selection_start = Some(Rc::new(start));
        self.on_selection_extend = Some(Rc::new(extend));
        self.on_selection_finish = Some(Rc::new(finish));
        self
    }

    fn coord(&self, byte_offset: usize) -> TextCoord {
        TextCoord {
            message_index: self.message_index,
            element_index: self.element_index,
            byte_offset,
        }
    }

    /// This element's own highlight range: fully highlighted if it sits strictly between the
    /// selection's two endpoints in document order, partially if it *is* one of the endpoints
    /// (sliced to the local byte offset), otherwise not highlighted at all.
    fn local_highlight_range(&self) -> Option<Range<usize>> {
        let (a, b) = self.selection_current?;
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };

        let my = (self.message_index, self.element_index);
        let lo_pos = (lo.message_index, lo.element_index);
        let hi_pos = (hi.message_index, hi.element_index);

        if my < lo_pos || my > hi_pos {
            return None;
        }

        let start = if my == lo_pos { lo.byte_offset } else { 0 };
        let end = if my == hi_pos {
            hi.byte_offset
        } else {
            self.full_text.len()
        };

        (start < end).then_some(start..end)
    }
}

impl IntoElement for SelectableRichText {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SelectableRichText {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

    fn id(&self) -> Option<ElementId> {
        Some(self.id.clone())
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(None, inspector_id, window, cx)
    }

    fn prepaint(
        &mut self,
        _global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Hitbox {
        self.text
            .prepaint(None, inspector_id, bounds, state, window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        global_id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut Self::RequestLayoutState,
        hitbox: &mut Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_view = window.current_view();
        let text_layout = self.text.layout().clone();
        let selection_color = self.selection_color;
        let selection_current = self.selection_current;
        // Plain values (not `&self`) so the mouse-event closures below, which must be
        // `'static`, can build a `TextCoord` without borrowing this element past `paint`.
        let message_index = self.message_index;
        let element_index = self.element_index;
        let my_coord = move |byte_offset: usize| TextCoord {
            message_index,
            element_index,
            byte_offset,
        };

        // Selection highlight paints *under* the text, drawn before `self.text.paint`.
        if let Some(range) = self.local_highlight_range() {
            for rect in selection_rects(&text_layout, &self.full_text, range, bounds) {
                window.paint_quad(fill(rect, selection_color));
            }
        }

        let mouse_position = window.mouse_position();
        let over_clickable = text_layout
            .index_for_position(mouse_position)
            .is_ok_and(|ix| self.click_ranges.iter().any(|r| r.contains(&ix)));
        if over_clickable {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        } else if hitbox.is_hovered(window) {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        // Mouse down: (re)start the shared selection at this position.
        if let Some(on_start) = self.on_selection_start.clone() {
            let hitbox = hitbox.clone();
            let text_layout = text_layout.clone();
            window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && hitbox.is_hovered(window)
                    && let Ok(ix) = text_layout.index_for_position(event.position)
                {
                    on_start(my_coord(ix), cx);
                }
            });
        }

        // Drag extension (regardless of which element the drag started in) while over me;
        // otherwise, local hover tracking for the cursor/underline-on-hover styling.
        {
            let hitbox = hitbox.clone();
            let text_layout = text_layout.clone();
            let hover_keys = self.hover_keys.clone();
            let click_ranges = self.click_ranges.clone();
            let on_hover = self.on_hover.clone();
            let on_extend = self.on_selection_extend.clone();
            window.with_optional_element_state::<HoverLocalState, _>(
                global_id,
                move |state, window| {
                    let state = state.flatten().unwrap_or_default();
                    let hovered_range = state.hovered_range.clone();

                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                        if phase != DispatchPhase::Bubble {
                            return;
                        }

                        if selection_current.is_some() {
                            if hitbox.is_hovered(window)
                                && let Some(on_extend) = &on_extend
                                && let Ok(ix) = text_layout.index_for_position(event.position)
                            {
                                on_extend(my_coord(ix), cx);
                            }
                            return;
                        }

                        let hovered = hitbox.is_hovered(window)
                            .then(|| {
                                text_layout
                                    .index_for_position(event.position)
                                    .ok()
                                    .and_then(|ix| {
                                        click_ranges.iter().position(|r| r.contains(&ix))
                                    })
                            })
                            .flatten();

                        if hovered_range.get() != hovered {
                            hovered_range.set(hovered);
                            if let Some(on_hover) = &on_hover {
                                on_hover(hovered.map(|ix| hover_keys[ix].clone()), window, cx);
                            }
                            cx.notify(current_view);
                        }
                    });

                    ((), Some(state))
                },
            );
        }

        // Mouse up: a click with no movement fires the run's action; any real selection gets
        // finalized (and copied to the clipboard) by `on_selection_finish`, which knows the
        // full anchor/cursor and every message they span - this element only knows itself.
        if let Some(on_finish) = self.on_selection_finish.clone() {
            let hitbox = hitbox.clone();
            let text_layout = text_layout.clone();
            let click_ranges = self.click_ranges.clone();
            let click_actions = self.click_actions.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase != DispatchPhase::Bubble || !hitbox.is_hovered(window) {
                    return;
                }
                let Ok(up_ix) = text_layout.index_for_position(event.position) else {
                    return;
                };
                let up_coord = my_coord(up_ix);

                let was_plain_click = selection_current
                    .is_none_or(|(anchor, cursor)| anchor == cursor && anchor == up_coord);

                if was_plain_click
                    && let Some(action) = click_ranges
                        .iter()
                        .position(|r| r.contains(&up_ix))
                        .and_then(|ix| click_actions.get(ix))
                {
                    match action {
                        RunAction::OpenUrl(href) => cx.open_url(href),
                    }
                }

                on_finish(cx);
            });
        }

        self.text
            .paint(None, inspector_id, bounds, &mut (), &mut (), window, cx);
    }
}

/// Splits `range` into one highlight rect per visual (wrapped) line it touches, so a selection
/// spanning a line wrap paints as a proper multi-line block instead of one rect corrupted by
/// the wrap. Built purely from `position_for_index` (sampling every char boundary and grouping
/// consecutive same-`y` points into rows) since `gpui` doesn't expose per-line byte ranges
/// directly - only their `unwrapped_layout` glyph indices, which isn't worth the translation
/// for a handful of short chat-message lines.
fn selection_rects(
    text_layout: &TextLayout,
    full_text: &str,
    range: Range<usize>,
    container: Bounds<Pixels>,
) -> Vec<Bounds<Pixels>> {
    if range.start >= range.end {
        return Vec::new();
    }

    let line_height = text_layout.line_height();

    // `text_layout.text()` joins wrapped lines with an inserted `\n` - a *different*, longer
    // string than the original, so its byte offsets don't line up with `range` (which is in
    // terms of the original unwrapped text, same as `position_for_index`). Walk `full_text`'s
    // own char boundaries instead.
    let mut boundaries: Vec<usize> = full_text
        .char_indices()
        .map(|(ix, _)| ix)
        .filter(|ix| *ix > range.start && *ix < range.end)
        .collect();
    boundaries.insert(0, range.start);
    boundaries.push(range.end);

    let mut rects = Vec::new();
    let mut row_start: Option<Point<Pixels>> = None;
    let mut prev: Option<Point<Pixels>> = None;

    for ix in boundaries {
        let Some(pos) = text_layout.position_for_index(ix.min(full_text.len())) else {
            continue;
        };

        match row_start {
            Some(start_pos) if pos.y != start_pos.y => {
                if let Some(end_pos) = prev {
                    rects.push(Bounds::from_corners(
                        start_pos,
                        point(container.right(), end_pos.y + line_height),
                    ));
                }
                row_start = Some(pos);
            }
            None => row_start = Some(pos),
            _ => {}
        }
        prev = Some(pos);
    }

    if let (Some(start_pos), Some(end_pos)) = (row_start, prev) {
        rects.push(Bounds::from_corners(
            start_pos,
            point(end_pos.x, end_pos.y + line_height),
        ));
    }

    rects
}
