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
    App, Bounds, GlobalElementId, Hsla, InspectorElementId, LayoutId, Pixels, Point, StyledText,
    Window, px, quad, size,
};

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
    type PrepaintState = ();

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
    ) {
        self.text.prepaint(id, inspector, bounds, state, window, cx)
    }

    fn paint(
        &mut self,
        id: Option<&GlobalElementId>,
        inspector: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
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
            .paint(id, inspector, bounds, request_layout, prepaint, window, cx)
    }
}

impl Wash {
    /// One box per span, or several when a span was wrapped across lines.
    fn boxes(&self, bounds: Bounds<Pixels>) -> Vec<Bounds<Pixels>> {
        let layout = self.text.layout();
        let line = layout.line_height();
        let breathe = line * BREATHE;
        let mut boxes = Vec::new();

        for span in &self.spans {
            let (Some(from), Some(to)) = (
                layout.position_for_index(span.start),
                layout.position_for_index(span.end),
            ) else {
                continue;
            };

            // A wrapped span is one box per row it lands on. Where the box ends on
            // a row that carries on is the element's own right edge rather than
            // the text's: a wrapped row's width is not something the layout
            // reports, and a span that wraps at all is the rare case.
            let mut top = from.y;
            let mut left = from.x - self.pad;
            while top < to.y {
                boxes.push(rect(left, top + breathe, bounds.right(), line - breathe * 2.0));
                top += line;
                left = bounds.left();
            }
            boxes.push(rect(
                left,
                top + breathe,
                to.x + self.pad,
                line - breathe * 2.0,
            ));
        }

        boxes
    }
}

fn rect(left: Pixels, top: Pixels, right: Pixels, height: Pixels) -> Bounds<Pixels> {
    Bounds {
        origin: Point { x: left, y: top },
        size: size((right - left).max(px(0.0)), height),
    }
}
