//! A rounded box behind a run of text.
//!
//! `HighlightStyle` is a colour over a range of characters: no radius, no border
//! and no padding, so inline code could only ever be a wash the shape of the
//! letters. This lays the text out exactly as it was — one layout, so a line
//! still wraps as a line rather than becoming a row of elements that wrap at
//! their own edges — and paints the boxes underneath it from the glyph positions
//! that layout settled on.
//!
//! The padding is the box's, not the text's. Widening the chip by inserting thin
//! spaces around the code was the other way to get it, and it puts characters
//! into the message that nobody typed: they land in a copy, they count towards
//! the length, and every offset after them is one more thing to keep in step.
//! Inflating the painted rectangle instead leaves the string alone. It overhangs
//! its neighbours by a fifth of an em, which is what a chip drawn around a word
//! in running text does everywhere else.

use std::ops::Range;

use gpui::{
    App, Bounds, CursorStyle, DispatchPhase, GlobalElementId, Hitbox, HitboxBehavior, Hsla,
    InspectorElementId, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Point, SharedString, StyledText, Window, px, quad, size,
};

use super::selection;

/// How much of the line's height the box leaves clear, top and bottom.
///
/// Discord's inline code is a fifth of an em of padding above and below the
/// text, which on a line box half again the size of the text leaves a chip a
/// quarter taller than what is in it. Filling the whole line box instead reads
/// as a highlighter pen.
const BREATHE: f32 = (1.0 - 1.25 / 1.5) / 2.0;

pub struct Wash {
    text: StyledText,
    /// Byte ranges of the laid-out text, not of the message body: the two differ
    /// wherever a mention was substituted or a spoiler replaced.
    spans: Vec<Range<usize>>,
    fill: Hsla,
    /// The hairline around the box, so inline code is drawn as the same object
    /// the fenced block is rather than as a wash that happens to be monospace.
    border: Hsla,
    radius: Pixels,
    /// How far the box reaches past the glyphs, left and right.
    pad: Pixels,
    /// `None` for the runs that are not text you can take -- a quote, which is a
    /// picture of something said elsewhere.
    selectable: Option<Selectable>,
}

/// What it takes to select a run: what it is called while it is selected, the
/// text of it, and what a selection in it is painted in.
///
/// The text is handed in rather than read back off the layout, which would be a
/// copy of every message on screen every frame.
#[derive(Clone)]
struct Selectable {
    id: SharedString,
    text: SharedString,
    tint: Hsla,
}

/// Text with a box behind each of `spans`. With no spans it is the text and
/// nothing else, so the caller never has to decide whether to wrap it.
pub fn wash(
    text: StyledText,
    spans: Vec<Range<usize>>,
    fill: Hsla,
    border: Hsla,
    radius: f32,
    pad: f32,
) -> Wash {
    Wash {
        text,
        spans,
        fill,
        border,
        radius: px(radius),
        pad: px(pad),
        selectable: None,
    }
}

impl Wash {
    /// Makes the run text you can take: a drag lights what it crosses, a double
    /// click a word, a third click the paragraph. The id has to be the run's own
    /// and stable across frames, since it is how the one selection there is says
    /// which run it belongs to.
    pub fn selectable(mut self, id: SharedString, text: SharedString, tint: Hsla) -> Self {
        self.selectable = Some(Selectable { id, text, tint });
        self
    }
}

impl gpui::IntoElement for Wash {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl gpui::Element for Wash {
    type RequestLayoutState = ();
    /// The box the pointer is tested against, for a run that can be selected.
    type PrepaintState = Option<Hitbox>;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        self.text.request_layout(id, inspector, window, cx)
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        state: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<Hitbox> {
        self.text.prepaint(id, inspector, bounds, state, window, cx);
        self.selectable
            .is_some()
            .then(|| window.insert_hitbox(bounds, HitboxBehavior::Normal))
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        hitbox: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        if let Some((of, hitbox)) = self.selectable.clone().zip(hitbox.clone()) {
            self.select(&of, bounds, &hitbox, window, cx);
        }

        for box_ in self.boxes(bounds) {
            window.paint_quad(quad(
                box_,
                self.radius,
                self.fill,
                px(1.0),
                self.border,
                gpui::BorderStyle::Solid,
            ));
        }

        self.text
            .paint(id, inspector, bounds, request_layout, &mut (), window, cx)
    }
}

impl Wash {
    /// Paints whatever is lit in this run, and listens for what would change it.
    ///
    /// The listeners are the window's rather than the element's, because a drag
    /// leaves the words it started in almost immediately: a `div` only hears a
    /// mouse move while the pointer is over it, and a selection that stopped
    /// growing at the edge of the paragraph would be a selection you cannot make
    /// backwards.
    fn select(
        &self,
        of: &Selectable,
        bounds: Bounds<Pixels>,
        hitbox: &Hitbox,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Selectable { id, text, tint } = of;
        let layout = self.text.layout().clone();

        if let Some(range) = selection::within(id, cx) {
            // Full line boxes and no padding: this is a selection, not a chip,
            // and every selection anywhere is the height of the line it is in.
            for box_ in self.spanning(bounds, &range, px(0.0), px(0.0)) {
                window.paint_quad(gpui::fill(box_, *tint));
            }
        }

        // The one affordance text you can take has: the pointer says so.
        window.set_cursor_style(CursorStyle::IBeam, hitbox);

        window.on_mouse_event({
            let (id, text, layout, hitbox) =
                (id.clone(), text.clone(), layout.clone(), hitbox.clone());
            move |event: &MouseDownEvent, phase, window: &mut Window, cx: &mut App| {
                if phase != DispatchPhase::Bubble
                    || event.button != MouseButton::Left
                    || !hitbox.is_hovered(window)
                {
                    return;
                }
                let at = layout.index_for_position(event.position).unwrap_or_else(|at| at);
                match event.click_count {
                    0 | 1 => selection::begin(id.clone(), text.clone(), at, cx),
                    2 => selection::lit(
                        id.clone(),
                        text.clone(),
                        selection::word(&text, at),
                        cx,
                    ),
                    // A third click is the paragraph, as it is in every browser
                    // and every editor.
                    _ => selection::lit(id.clone(), text.clone(), 0..text.len(), cx),
                }
                window.refresh();
            }
        });

        window.on_mouse_event({
            let (id, layout) = (id.clone(), layout.clone());
            move |event: &MouseMoveEvent, phase, window: &mut Window, cx: &mut App| {
                if phase != DispatchPhase::Bubble || !selection::dragging(&id, cx) {
                    return;
                }
                // A mouse released outside the window is a release there is no
                // event for, and a drag that never ended would go on growing
                // under a pointer with nothing held down. A move with the button
                // up is that release, late.
                if !event.dragging() {
                    selection::release(cx);
                    return;
                }
                selection::extend(
                    layout.index_for_position(event.position).unwrap_or_else(|at| at),
                    cx,
                );
                window.refresh();
            }
        });

        window.on_mouse_event({
            let id = id.clone();
            move |_: &MouseUpEvent, phase, window: &mut Window, cx: &mut App| {
                if phase == DispatchPhase::Bubble && selection::dragging(&id, cx) {
                    selection::release(cx);
                    window.refresh();
                }
            }
        });
    }

    /// One box per span, or several when a span was wrapped across lines.
    fn boxes(&self, bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
        let breathe = self.text.layout().line_height() * BREATHE;

        self.spans
            .iter()
            .flat_map(|span| self.spanning(bounds, span, self.pad, breathe))
            .collect()
    }

    /// The rectangles a range of the laid-out text covers: one, or one per row it
    /// wrapped onto.
    fn spanning(
        &self,
        bounds: Bounds<Pixels>,
        span: &Range<usize>,
        pad: Pixels,
        breathe: Pixels,
    ) -> Vec<Bounds<Pixels>> {
        let layout = self.text.layout();
        let line = layout.line_height();
        let (Some(from), Some(to)) = (
            layout.position_for_index(span.start),
            layout.position_for_index(span.end),
        ) else {
            return Vec::new();
        };

        // A wrapped span is one box per row it lands on. Where the box ends on a
        // row that carries on is the element's own right edge rather than the
        // text's: a wrapped row's width is not something the layout reports.
        let mut boxes = Vec::new();
        let mut top = from.y;
        let mut left = from.x - pad;
        while top < to.y {
            boxes.push(rect(left, top + breathe, bounds.right(), line - breathe * 2.0));
            top += line;
            left = bounds.left();
        }
        boxes.push(rect(left, top + breathe, to.x + pad, line - breathe * 2.0));

        boxes
    }
}

fn rect(left: Pixels, top: Pixels, right: Pixels, height: Pixels) -> Bounds<Pixels> {
    Bounds {
        origin: Point { x: left, y: top },
        size: size((right - left).max(px(0.0)), height),
    }
}
