//! An equation laid out, rather than transliterated.
//!
//! `latex::flatten` reads a fraction as `a⁄b`, which is what one line of text
//! can say about it and is what everything outside the conversation gets. Here
//! there is a box model to spend: a fraction is a column with a rule across it,
//! a script is a smaller run set against the top or the bottom of its base, a
//! radical has a bar over what is under it, and a ∑ carries its limits above
//! and below rather than beside.
//!
//! Only display maths (`$$…$$`) is drawn this way. An equation inside a
//! sentence has to be part of the sentence's own text run or the line stops
//! wrapping at it, and gpui has no inline box — so `$…$` keeps the reading.
//! Which is also the split TeX draws: inline fractions are set on a slash in
//! most houses for exactly the same reason, that a line of prose has one
//! height.
//!
//! Everything is sized off one number, the size the equation is set at, so the
//! whole thing scales with the conversation's typography.

use gpui::prelude::*;
use gpui::{AnyElement, Div, SharedString, div, px};

use petunia_config::Theme;
use petunia_data::message::latex::Node;

/// How much smaller a script, a limit or a fraction's half is set than what it
/// hangs off. TeX's own script size is 70% and its script-script 50%; a chat
/// window is not a page and the second step is unreadable at body size, so
/// there is one step and it is gentler.
const SMALLER: f32 = 0.78;

/// The thinnest rule that still draws at every scale factor.
const RULE: f32 = 1.0;

/// An equation as elements. `size` is the point size the surrounding text is
/// set at; everything else is a fraction of it.
pub fn typeset(nodes: &[Node], theme: &Theme, size: f32) -> AnyElement {
    row(nodes, theme, size).into_any_element()
}

/// A sequence, laid out left to right on one baseline. `items_baseline` is not
/// something gpui offers, so the row is centred — which for a line of maths,
/// where the tall things are fractions and radicals that are symmetric about
/// their own middle, is where the baseline would have put them anyway.
fn row(nodes: &[Node], theme: &Theme, size: f32) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .children(nodes.iter().map(|node| one(node, theme, size)))
}

fn one(node: &Node, theme: &Theme, size: f32) -> AnyElement {
    match node {
        Node::Run { text, slanted } => run(text, *slanted, theme, size).into_any_element(),
        Node::Group(inner) => row(inner, theme, size).into_any_element(),
        Node::Frac(numerator, denominator) => {
            fraction(numerator, denominator, theme, size).into_any_element()
        }
        Node::Sqrt(radicand) => radical(radicand, theme, size).into_any_element(),
        Node::Script { base, over, under } => {
            script(base, over.as_deref(), under.as_deref(), theme, size).into_any_element()
        }
        Node::Big { glyph, over, under } => {
            big(glyph, over.as_deref(), under.as_deref(), theme, size).into_any_element()
        }
        Node::Fenced { open, close, inner } => {
            fenced(open, close, inner, theme, size).into_any_element()
        }
        Node::Accent { mark, inner } => accent(*mark, inner, theme, size).into_any_element(),
        // A space is a space of this equation's size rather than of the
        // interface's, or the gaps in a large display equation stay small.
        Node::Space(width) => div()
            .flex_none()
            .w(px(size * 0.3 * *width as f32))
            .into_any_element(),
        // A line break inside a row cannot break it, so it is a gap wide enough
        // to read as one. An equation that wants two lines wants two `$$`.
        Node::Break => div().flex_none().w(px(size)).into_any_element(),
    }
}

/// Letters, digits and glyphs. Italic where the variable rule says so, upright
/// everywhere else — an italic ∑ or an italic `sin` is a mark nobody sets that
/// way, and slanting the whole equation is what made one read as a sentence in
/// italics that happened to contain symbols.
fn run(text: &str, slanted: bool, theme: &Theme, size: f32) -> Div {
    div()
        .flex_none()
        .whitespace_nowrap()
        .text_size(px(size))
        .text_color(theme.text)
        .when(slanted, |this| this.italic())
        .child(SharedString::from(text.to_owned()))
}

/// A numerator over a denominator with a rule between them. Both halves are set
/// a step down, which is what stops a fraction inside a fraction from being the
/// same size as the equation around it.
fn fraction(numerator: &[Node], denominator: &[Node], theme: &Theme, size: f32) -> Div {
    let inner = size * SMALLER;

    div()
        .flex()
        .flex_none()
        .flex_col()
        .items_center()
        // Room either side of the rule, so it is not touching either half, and
        // room outside so a fraction beside a letter is not against it.
        .mx(px(size * 0.15))
        .child(div().px(px(size * 0.2)).child(row(numerator, theme, inner)))
        .child(
            div()
                .w_full()
                .h(px(RULE))
                .my(px(size * 0.08))
                .bg(theme.text),
        )
        .child(div().px(px(size * 0.2)).child(row(denominator, theme, inner)))
}

/// The radical sign, and a bar over what is under it — which is the half of a
/// root a single glyph cannot draw, and the half that says where the root ends.
fn radical(radicand: &[Node], theme: &Theme, size: f32) -> Div {
    div()
        .flex()
        .flex_none()
        .items_stretch()
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .text_size(px(size * 1.1))
                .text_color(theme.text)
                .child("√"),
        )
        .child(
            div()
                .flex()
                .items_center()
                .border_t(px(RULE))
                .border_color(theme.text)
                .pt(px(size * 0.08))
                .px(px(size * 0.1))
                .child(row(radicand, theme, size)),
        )
}

/// A base with what is raised or lowered against it.
///
/// One of each gets a single smaller run set against the top or the bottom of
/// the base; both get a column, which is the only arrangement that keeps them
/// from overlapping. `items_start` and `items_end` are what do the raising:
/// there is no baseline to offset from, so the script is aligned against the
/// edge of the base's own line box.
fn script(
    base: &[Node],
    over: Option<&[Node]>,
    under: Option<&[Node]>,
    theme: &Theme,
    size: f32,
) -> Div {
    let small = size * SMALLER;
    let body = row(base, theme, size);

    match (over, under) {
        (Some(over), Some(under)) => div()
            .flex()
            .flex_none()
            .items_stretch()
            .child(body)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .justify_between()
                    .ml(px(size * 0.05))
                    .child(row(over, theme, small))
                    .child(row(under, theme, small)),
            ),
        (Some(over), None) => div()
            .flex()
            .flex_none()
            .items_start()
            .child(body)
            .child(div().ml(px(size * 0.05)).child(row(over, theme, small))),
        (None, Some(under)) => div()
            .flex()
            .flex_none()
            .items_end()
            .child(body)
            .child(div().ml(px(size * 0.05)).child(row(under, theme, small))),
        (None, None) => div().flex().flex_none().child(body),
    }
}

/// A ∑ with its limits stacked on it. Drawn larger than the equation around it,
/// the way every maths font draws the display form: a summation sign at body
/// size beside two limits at script size is a symbol smaller than its own
/// annotations.
fn big(
    glyph: &str,
    over: Option<&[Node]>,
    under: Option<&[Node]>,
    theme: &Theme,
    size: f32,
) -> Div {
    let small = size * SMALLER * SMALLER;

    div()
        .flex()
        .flex_none()
        .flex_col()
        .items_center()
        .mx(px(size * 0.12))
        .children(over.map(|over| row(over, theme, small)))
        .child(
            div()
                .flex_none()
                .whitespace_nowrap()
                .text_size(px(size * 1.6))
                .text_color(theme.text)
                .child(SharedString::from(glyph.to_owned())),
        )
        .children(under.map(|under| row(under, theme, small)))
}

/// A bracket that knows what it is holding. Stretched to the height of it,
/// which is the one thing a `\left…\right` pair is for and the one thing a
/// bracket typed on its own cannot be.
fn fenced(open: &str, close: &str, inner: &[Node], theme: &Theme, size: f32) -> Div {
    let bracket = |glyph: &str| {
        div()
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .text_size(px(size))
            .text_color(theme.text)
            // The height a bracket is stretched to is its parent's, and the
            // parent is as tall as what is between the brackets.
            .h_full()
            .child(SharedString::from(glyph.to_owned()))
    };

    div()
        .flex()
        .flex_none()
        .items_stretch()
        .when(!open.is_empty(), |this| this.child(bracket(open)))
        .child(row(inner, theme, size))
        .when(!close.is_empty(), |this| this.child(bracket(close)))
}

/// A hat, a bar or an arrow over what it marks. The combining mark where there
/// is one character to combine with, and a rule of the right width where there
/// is not — which is what a bar over a group actually is, and what a combining
/// mark could never be.
fn accent(mark: char, inner: &[Node], theme: &Theme, size: f32) -> Div {
    let body = row(inner, theme, size);
    let single = matches!(inner, [Node::Run { text, .. }] if text.chars().count() == 1);

    if single {
        let Some(Node::Run { text, slanted }) = inner.first() else {
            return body;
        };
        return div()
            .flex()
            .flex_none()
            .child(run(&format!("{text}{mark}"), *slanted, theme, size));
    }

    div()
        .flex()
        .flex_none()
        .flex_col()
        .items_center()
        .child(
            div()
                .w_full()
                .h(px(RULE))
                .mb(px(size * 0.06))
                .bg(theme.text),
        )
        .child(body)
}
