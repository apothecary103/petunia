//! Petunia's own primitives. The widget library supplies the window root and
//! the text input; everything with a visual opinion is built here so the look
//! is one decision rather than a per-view accident.

use gpui::prelude::*;
use gpui::{App, Div, Hsla, MouseButton, SharedString, Stateful, Window, div, px};
use gpui_component::{Icon, IconName};

use petunia_config::Theme;

/// An icon at a weight that reads as chrome rather than as content.
pub fn icon(name: IconName, size: f32, tint: Hsla) -> Icon {
    Icon::new(name).size(px(size)).text_color(tint)
}

/// One of petunia's own icons, which the library's set does not ship. Named by
/// the path `crate::assets` serves it at.
pub fn glyph(path: &'static str, size: f32, tint: Hsla) -> Icon {
    Icon::empty().path(path).size(px(size)).text_color(tint)
}

/// The rounding used everywhere. One value, so nothing drifts.
pub const RADIUS: f32 = 8.0;
pub const RADIUS_LG: f32 = 12.0;

/// The two weights above regular that macOS actually draws.
///
/// gpui asks font-kit for a CSS weight and font-kit matches per CSS Fonts 3.
/// The system family reports its faces at CoreText's own weights, and the
/// conversion lands Medium on 530 rather than 500 -- so a request for 500 finds
/// no exact match, hits the rule that checks 400 first, and rasterises as
/// Regular. `FontWeight::MEDIUM` is therefore invisible here. Semibold (600) and
/// Bold (700) land exactly, and are what AppKit itself hands out for emphasis.
pub const EMPHASIS: gpui::FontWeight = gpui::FontWeight::SEMIBOLD;
pub const STRONG: gpui::FontWeight = gpui::FontWeight::BOLD;

/// What everything that is not emphasised is set at, applied once at the root of
/// the window so no view has to remember it.
///
/// Regular, which is what the Human Interface Guidelines say body text is and
/// what AppKit sets every list row, label and control title in. Weight in that
/// system is *hierarchy*, not texture: Semibold is what a headline is made of,
/// and a window that raises the floor has not made itself clearer, it has thrown
/// the ladder away.
///
/// Medium — the one step between — cannot be had here. Asking for 500 gets
/// Regular, per the note above; asking for 510 walks the heavier faces first and
/// gets Semibold, which is how this briefly set the entire application in
/// Semibold. So the ladder on macOS is Regular, Semibold, Bold, and everything
/// below Semibold does its work with size and colour instead.
pub const BODY: gpui::FontWeight = gpui::FontWeight::NORMAL;

/// How far a row's content sits from the edge of the column it is in: the list's
/// own padding, the row's, and the outline every row keeps so selecting one never
/// shifts it. Anything else pinned to that column lines up with this rather than
/// picking a padding of its own and landing a few pixels out.
pub const INSET: f32 = LIST_PADDING + ROW_PADDING + 1.0;
/// What a column of rows keeps clear at its own edges.
///
/// Roomier than it was, and so is the padding inside a row. A conversation list
/// is read down rather than across: the space between the rows is what makes it
/// scannable, and at eight and ten it was a table of contents.
pub const LIST_PADDING: f32 = 12.0;
const ROW_PADDING: f32 = 10.0;

/// A row that can be picked: quiet by default, filled when it is the one you
/// are on.
///
/// The fill is the whole mark — a grey a clear step above the hover, as Signal
/// draws it, and nothing else. An accent tint with a hairline and a stripe was
/// legible from across the room and looked like a selected *cell* rather than
/// the conversation you happen to be in; the eye finds one filled row in a
/// column of unfilled ones without being shouted at.
///
/// The border stays, transparent, so `INSET` is one number and selecting a row
/// never moves it.
pub fn row(id: impl Into<gpui::ElementId>, selected: bool, theme: &Theme) -> Stateful<Div> {
    div()
        .id(id)
        .relative()
        .flex()
        .items_start()
        .gap_2p5()
        .px(px(ROW_PADDING))
        .py(px(ROW_PADDING - 1.0))
        .rounded(px(RADIUS))
        .border_1()
        .border_color(gpui::transparent_black())
        .when(selected, |this| this.bg(theme.active))
        .when(!selected, |this| this.hover(|this| this.bg(theme.hover)))
}

/// A section label. Small, muted, and never competing with the rows under it.
/// Upper case, because gpui has no letter-spacing and case is the only other
/// thing that makes a line this small read as a heading rather than as text.
pub fn section(label: impl Into<SharedString>, theme: &Theme) -> Div {
    heading(None, label, theme)
}

/// A section label with an icon, for the ones that are a kind of thing rather
/// than simply the rest.
pub fn heading(icon: Option<IconName>, label: impl Into<SharedString>, theme: &Theme) -> Div {
    let label = label.into();

    div()
        .flex()
        .items_center()
        .gap_1p5()
        // A heading is not a row, so it has no outline to sit inside. The extra
        // pixel on the left is what lines it up with the rows under it.
        .pl(px(ROW_PADDING + 1.0))
        .pr(px(ROW_PADDING))
        .pt_3()
        .pb_1p5()
        .when_some(icon, |this, icon| {
            this.child(
                div()
                    .flex_none()
                    .flex()
                    .items_center()
                    .child(self::icon(icon, 11.0, theme.text_muted)),
            )
        })
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                // Eleven points, which is the Guidelines' footnote and the smallest
                // size AppKit sets a label at. Ten was a size this window used
                // nowhere else and could not be read across a desk.
                .text_size(px(theme.typography.ui_size - 2.0))
                .font_weight(EMPHASIS)
                .text_color(theme.text_muted)
                .child(SharedString::from(label.to_uppercase())),
        )
}

/// A square, quiet, hover-lit control. The reference has no bordered buttons in
/// its chrome, only glyphs that light up.
pub fn icon_button(
    id: impl Into<gpui::ElementId>,
    name: IconName,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    press(id, theme, on_click).child(icon(name, 15.0, theme.text_muted))
}

/// The same control with one of petunia's own glyphs in it.
pub fn glyph_button(
    id: impl Into<gpui::ElementId>,
    path: &'static str,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    press(id, theme, on_click).child(glyph(path, 16.0, theme.text_muted))
}

/// The box both of them are: a soft square that lights under the pointer, round
/// enough to belong beside the cards and the pills rather than to a toolbar.
fn press(
    id: impl Into<gpui::ElementId>,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .size(px(28.0))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(8.0))
        .hover(|this| this.bg(theme.hover))
        .on_mouse_down(MouseButton::Left, on_click)
}

/// A small rounded label. Used for counts, states and anything annotating
/// something else rather than standing alone.
pub fn chip(label: impl Into<SharedString>, tint: Hsla, theme: &Theme) -> Div {
    chip_sized(label, tint, theme.typography.ui_size - 4.0)
}

/// The same chip at a size the caller picks.
fn chip_sized(label: impl Into<SharedString>, tint: Hsla, size: f32) -> Div {
    div()
        .flex_none()
        .px_1p5()
        .rounded_full()
        .bg(tinted(tint))
        .text_size(px(size))
        .text_color(tint)
        .child(label.into())
}

/// A count that is meant to be seen: filled rather than tinted, and lettered in
/// the colour that reads on the fill.
///
/// Every chat list draws its unread count this way, Signal included, and the
/// reason is that this is the one number in the window that is asking to be
/// looked at. A tinted chip is how petunia annotates something — quiet, taking
/// the colour of the text beside it — which for "there are nine messages you have
/// not read" was a grey number saying nothing louder than the preview under it.
pub fn count(label: impl Into<SharedString>, fill: Hsla, on: Hsla, theme: &Theme) -> Div {
    let size = theme.typography.ui_size - 3.0;

    div()
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        // Wide enough that a single digit is a circle rather than a lozenge, and
        // free to grow past that for three.
        .min_w(px(size * 1.5))
        .px_1()
        .rounded_full()
        .bg(fill)
        .text_size(px(size))
        .line_height(px(size * 1.5))
        .font_weight(EMPHASIS)
        .text_color(on)
        .child(label.into())
}

/// A badge: what a group says about somebody, beside their name.
///
/// Not `chip`. A chip is pill-shaped for a count, where the shape is the point;
/// a badge sits in a line of text, so it is squared off to the same corner radius
/// as everything else and given real vertical padding. Its line height is set to
/// the text beside it so a row of `items_center` puts them on the same line --
/// left to the font, a chip's box is taller than the name it annotates and the
/// two never look aligned.
pub fn badge(label: impl Into<SharedString>, tint: Hsla, size: f32, line: f32) -> Div {
    div()
        .flex_none()
        .px(px(size * 0.45))
        .rounded(px(4.0))
        .bg(tinted(tint))
        .text_size(px(size))
        .line_height(px(line))
        .font_weight(EMPHASIS)
        .text_color(tint)
        .child(label.into())
}

/// How far a message of ours got, drawn the way Signal draws it: a check in a
/// ring once it is sent, a second ring beside it once it is delivered, and both
/// rings filled once it has been read.
///
/// Bare ticks were the wrong alphabet. Signal's mark is a *circled* check, and
/// what changes between delivered and read is the fill rather than the colour —
/// which is why nothing here is tinted differently per state: the shape is the
/// difference, and the two overlapping rings only read as a pair because the one
/// in front knocks a gap out of the one behind. That gap is a `clipPath` in the
/// asset, since it cannot be had by drawing one glyph twice.
///
/// Here rather than in the conversation, because the list draws the same mark on
/// the line it shows and one shape said twice is two shapes that can disagree.
pub fn receipt(mark: Mark, size: f32, tint: Hsla) -> Div {
    let (path, wide) = match mark {
        Mark::Sent => ("icons/receipt-sent.svg", false),
        Mark::Delivered => ("icons/receipt-delivered.svg", true),
        Mark::Read => ("icons/receipt-read.svg", true),
    };
    // The doubles are an eighteen-by-twelve box, so asking for a square squashes
    // them. The height is what has to match the text beside it.
    let width = match wide {
        true => size * 1.5,
        false => size,
    };

    div().flex().flex_none().items_center().child(
        gpui::svg()
            .path(path)
            .w(px(width))
            .h(px(size))
            .text_color(tint),
    )
}

/// The three shapes `receipt` draws. Not `Status`: sending and failing are not
/// receipts, and each view says what it wants for those itself.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mark {
    Sent,
    Delivered,
    Read,
}


/// Three dots rising in turn: somebody is typing.
///
/// One animation for all three, with each dot reading the cycle a third of a turn
/// behind the last — three animations would drift apart, since each is timed from
/// the frame it first appeared on.
pub fn typing(size: f32, tint: Hsla, id: impl Into<SharedString>) -> gpui::AnimationElement<Div> {
    use gpui::AnimationExt as _;

    const DOTS: usize = 3;
    let id = id.into();

    div()
        .flex()
        .flex_none()
        .items_center()
        .gap(px(size * 0.5))
        .with_animation(
            gpui::ElementId::Name(id),
            gpui::Animation::new(std::time::Duration::from_millis(1_200)).repeat(),
            move |this, delta| {
                this.children((0..DOTS).map(|at| {
                    let phase = (delta + 1.0 - at as f32 / DOTS as f32) % 1.0;
                    // A rise and a rest, so the three read as a wave rather than
                    // as three things blinking.
                    let lifted = (phase * std::f32::consts::TAU).sin().max(0.0);
                    div()
                        .size(px(size))
                        .rounded_full()
                        .bg(tint)
                        .opacity(0.35 + 0.65 * lifted)
                }))
            },
        )
}

/// The conversation column, which is the whole of the space between the panels.
///
/// It was capped at a reading measure, the way prose is set. A chat log is not
/// prose: a wide window is a window somebody made wide, and the messages in it
/// carry pictures, code and quotes that a narrow column only makes worse. Left
/// alone it also never moves sideways, which is what a cap centred in its column
/// would have done every time a panel opened.
pub fn column() -> Div {
    div().w_full()
}

/// A colour at the strength a fill wants rather than the strength text wants.
pub fn tinted(color: Hsla) -> Hsla {
    Hsla { a: 0.16, ..color }
}

/// What selected text is washed in: the accent worn thin, at the very strength
/// the text input's own selection uses (`theme::install`), so selecting words in
/// a message and selecting them in the composer are one gesture rather than two
/// that happen to look different.
pub fn selection(theme: &Theme) -> Hsla {
    Hsla {
        a: 0.30,
        ..theme.accent
    }
}

/// The box a line of text is typed into. The box rather than the input, so a
/// field made of more than one input -- a username, which is a name and a number
/// -- is one box with a separator drawn in it rather than two sitting beside each
/// other.
pub fn field(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .px_2p5()
        .py_2()
        .rounded(px(RADIUS))
        .bg(theme.sunken)
        .border_1()
        .border_color(theme.border)
}

/// The card a dialog sits in, over a scrim that dims what it is covering.
/// Clicking the scrim is how a dialog is dismissed, so the card stops the click
/// from reaching it.
pub fn dialog(width: f32, theme: &Theme) -> Div {
    div()
        .w(px(width))
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .rounded(px(RADIUS_LG))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

/// What covers the window while a dialog is up.
///
/// `occlude` is what makes it cover: gpui deliberately lets a scroll wheel
/// through to whatever scrollable is beneath, so that an overlay does not stop
/// the page under it from scrolling. Here that is exactly wrong -- the
/// conversation behind a sheet is out of reach, and scrolling it while reading
/// the sheet is not something anybody asked for.
pub fn scrim(theme: &Theme) -> Div {
    div()
        .absolute()
        .inset_0()
        .occlude()
        .flex()
        .items_center()
        .justify_center()
        .bg(Hsla {
            a: 0.6,
            ..theme.background
        })
}

/// How a button in a dialog reads. `Danger` is filled in the warning colour
/// rather than the accent, because the accent is what "send" is and a delete must
/// not look like the safe thing to click.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    Primary,
    Danger,
    Quiet,
}

pub fn button(
    id: impl Into<gpui::ElementId>,
    label: impl Into<SharedString>,
    intent: Intent,
    theme: &Theme,
    on_click: impl Fn(&gpui::MouseDownEvent, &mut Window, &mut App) + 'static,
) -> Stateful<Div> {
    let fill = match intent {
        Intent::Primary => Some(theme.accent),
        Intent::Danger => Some(theme.danger),
        Intent::Quiet => None,
    };

    div()
        .id(id)
        .px_3()
        .py_1p5()
        .rounded(px(RADIUS))
        .cursor_pointer()
        .when_some(fill, |this, fill| this.bg(fill))
        .when(fill.is_none(), |this| {
            this.border_1()
                .border_color(theme.border)
                .hover(|this| this.bg(theme.hover))
        })
        .text_size(px(theme.typography.ui_size))
        .text_color(match fill {
            Some(_) => theme.on_accent,
            None => theme.text_dim,
        })
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label.into())
}
