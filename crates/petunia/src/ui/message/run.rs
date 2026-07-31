//! One consecutive stretch from one sender, in whichever shape is configured.
//!
//! The three layouts are three arrangements of the same parts -- an avatar, a
//! name, badges, a clock, and the bodies -- so they are built from shared pieces
//! here rather than by three copies of the message list.

use gpui::prelude::*;
use gpui::{Div, Hsla, MouseButton, SharedString, Window, div, px, relative};
use gpui_component::highlighter::HighlightTheme;
use uuid::Uuid;

use super::act::{Act, Dispatch};
use super::content;
use crate::ui::avatar::avatar;
use crate::ui::kit;
use crate::ui::relative as when;
use petunia_config::Theme;
use petunia_config::messages::{Layout, Spacing, Timestamps};
use petunia_data::{Member, Message, State, Thread};
use petunia_media::audio::Playback;

/// How wide a bubble is allowed to get. Signal's own proportion: wide enough for
/// a paragraph, narrow enough that the side it is on is still obvious.
const BUBBLE: f32 = 0.78;

/// Everything one run draws.
pub struct Run<'a> {
    pub sender: Uuid,
    pub messages: &'a [Message],
    pub state: &'a State,
    /// The thread it was said in, for what the group says about the sender.
    pub thread: &'a Thread,
    pub theme: &'a Theme,
    pub highlights: &'a HighlightTheme,
    pub layout: Layout,
    pub spacing: Spacing,
    pub timestamps: Timestamps,
    pub max_image: (f32, f32),
    pub playback: &'a Playback,
    /// The message a search jumped to, lit so the answer is findable in the page
    /// it landed on.
    pub revealed: Option<u64>,
    pub act: &'a Dispatch,
}

impl Run<'_> {
    pub fn render(self) -> Div {
        match self.layout {
            Layout::Standard => self.standard(),
            Layout::Compact => self.compact(),
            Layout::Bubbles => self.bubbles(),
        }
    }

    /// An avatar gutter, one header per run, and the bodies hanging under it.
    fn standard(&self) -> Div {
        div()
            .flex()
            .items_start()
            .gap(px(self.spacing.gutter - self.spacing.avatar))
            .child(self.portrait(self.spacing.avatar))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap(px(self.spacing.within_run))
                    .children(self.header(true))
                    .children(self.bodies()),
            )
    }

    /// One line per message: the clock, the name in a column of its own, then
    /// what was said. Both columns are fixed and right-aligned, which is what
    /// makes every line of text start at the same place.
    fn compact(&self) -> Div {
        let tint = self.tint();

        div()
            .flex()
            .flex_col()
            .gap(px(self.spacing.within_run))
            .children(self.messages.iter().map(|message| {
                div()
                    .flex()
                    .items_start()
                    .gap_2()
                    .children(self.clock(message).map(|clock| {
                        div()
                            .flex_none()
                            .w(px(self.spacing.clock_column))
                            .text_right()
                            .text_size(px(self.spacing.small))
                            .line_height(px(self.line()))
                            .text_color(self.theme.text_muted)
                            .child(clock)
                    }))
                    .child(
                        div()
                            .id(SharedString::from(format!("nick-{}", message.timestamp())))
                            .flex_none()
                            .w(px(self.spacing.name_column))
                            .truncate()
                            .text_right()
                            .text_size(px(self.spacing.body))
                            .line_height(px(self.line()))
                            .font_weight(kit::EMPHASIS)
                            .text_color(tint)
                            .cursor_pointer()
                            .hover(|this| this.underline())
                            .on_mouse_down(MouseButton::Left, self.inspect())
                            .on_mouse_down(MouseButton::Right, self.menu())
                            .child(SharedString::from(self.name())),
                    )
                    .child(div().flex_1().min_w_0().child(self.body(message)))
            }))
    }

    /// Signal's own: what you said on the right, what they said on the left,
    /// each in a rounded bubble. No avatar on your own side, because there is
    /// only ever one person on it.
    fn bubbles(&self) -> Div {
        let own = self.own();
        let side = div()
            .flex()
            .flex_col()
            .flex_1()
            .min_w_0()
            .gap(px(self.spacing.within_run))
            .when(own, |this| this.items_end())
            .children(self.header(!own))
            .children(
                self.messages
                    .iter()
                    .map(|message| self.bubble(own, self.body(message))),
            );

        div()
            .flex()
            .items_end()
            .gap(px(self.spacing.gutter - self.spacing.avatar))
            // An avatar only where it distinguishes anybody: in a group, on the
            // messages that are not yours.
            .when(!own && self.thread.is_group(), |this| {
                this.child(self.portrait(self.spacing.avatar))
            })
            .child(side)
    }

    fn bubble(&self, own: bool, body: impl IntoElement) -> Div {
        div()
            .max_w(relative(BUBBLE))
            .px(px(self.spacing.bubble_x))
            .py(px(self.spacing.bubble_y))
            .rounded(px(kit::RADIUS_LG))
            .bg(match own {
                true => kit::tinted(self.theme.accent),
                false => self.theme.surface,
            })
            .child(body)
    }

    /// The name, what the group says about them, and when they started talking.
    /// `named` is false for your own side of a bubble layout, where the position
    /// already says who it was -- and with no name and no clock there is nothing
    /// to draw, so a run does not carry an empty line above it.
    fn header(&self, named: bool) -> Option<Div> {
        let tint = self.tint();
        let clock = self
            .messages
            .first()
            .and_then(|message| self.clock(message));
        if !named && clock.is_none() {
            return None;
        }

        // Centred rather than baseline-aligned. A badge is a box with a fill, and
        // its baseline is not the baseline of the text inside it, so aligning the
        // two on it leaves the badge sitting low beside the name.
        Some(
            div()
                .flex()
                .items_center()
                .gap_2()
                .when(named, |this| {
                    this.child(
                        div()
                            .id("sender")
                            .cursor_pointer()
                            .text_size(px(self.spacing.body))
                            .line_height(px(self.line()))
                            .font_weight(kit::EMPHASIS)
                            .text_color(tint)
                            .hover(|this| this.underline())
                            .on_mouse_down(MouseButton::Left, self.inspect())
                            .on_mouse_down(MouseButton::Right, self.menu())
                            .child(SharedString::from(self.name())),
                    )
                    .children(self.badges(tint))
                })
                .children(clock.map(|clock| {
                    div()
                        .text_size(px(self.spacing.small))
                        .line_height(px(self.line()))
                        .text_color(self.theme.text_muted)
                        .child(clock)
                })),
        )
    }

    /// What the group says about whoever is talking: the label they picked for
    /// themselves, then the role the group gave them. Empty outside a group, and
    /// empty for the ordinary members of one, so a badge here always means
    /// something.
    ///
    /// The label is tinted in the sender's own colour, because a label somebody
    /// picked for themselves belongs to them the way their name does; the role is
    /// the group's word rather than theirs, and stays neutral.
    fn badges(&self, tint: Hsla) -> Vec<Div> {
        let Some(member): Option<&Member> = self.state.member(self.thread, self.sender) else {
            return Vec::new();
        };
        let size = self.spacing.small;
        let line = self.line();

        member
            .badge()
            .map(|badge| kit::badge(badge, tint, size, line))
            .into_iter()
            .chain(
                member
                    .role
                    .label()
                    .map(|role| kit::badge(role, self.theme.text_dim, size, line)),
            )
            .collect()
    }

    fn bodies(&self) -> Vec<gpui::AnyElement> {
        self.messages
            .iter()
            .map(|message| self.body(message).into_any_element())
            .collect()
    }

    /// One message, lit when it is the one a search was looking for.
    fn body(&self, message: &Message) -> Div {
        let found = self.revealed == Some(message.timestamp());
        let body = content::Body {
            message,
            state: self.state,
            theme: self.theme,
            highlights: self.highlights,
            spacing: self.spacing,
            max_image: self.max_image,
            playback: self.playback,
            act: self.act,
        }
        .render();

        div()
            .when(found, |this| {
                this.px_1p5()
                    .rounded(px(kit::RADIUS))
                    .bg(kit::tinted(self.theme.warning))
            })
            .child(body)
    }

    /// The sender's picture, which opens their profile and carries their menu.
    fn portrait(&self, size: f32) -> gpui::Stateful<Div> {
        div()
            .id("sender-avatar")
            .flex_none()
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, self.inspect())
            .on_mouse_down(MouseButton::Right, self.menu())
            .child(avatar(
                self.state.avatar_for(self.sender),
                &self.name(),
                self.sender.as_bytes(),
                size,
                self.theme,
            ))
    }

    fn name(&self) -> String {
        self.state.sender_name(self.sender)
    }

    fn tint(&self) -> Hsla {
        self.theme.accent_for(self.sender.as_bytes())
    }

    fn own(&self) -> bool {
        self.sender == self.state.aci
    }

    /// The line every part of a header shares, so the name, the badges and the
    /// clock all sit on it however different their own text sizes are.
    fn line(&self) -> f32 {
        self.spacing.body * self.theme.typography.line_height
    }

    fn clock(&self, message: &Message) -> Option<SharedString> {
        (self.timestamps != Timestamps::Never).then(|| {
            SharedString::from(
                when::local(message.timestamp())
                    .map(|at| at.format("%H:%M").to_string())
                    .unwrap_or_default(),
            )
        })
    }

    fn inspect(&self) -> impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + use<> {
        let (act, sender) = (self.act.clone(), self.sender);
        move |_, window, cx| act(Act::Inspect(sender), window, cx)
    }

    fn menu(&self) -> impl Fn(&gpui::MouseDownEvent, &mut Window, &mut gpui::App) + use<> {
        let (act, sender) = (self.act.clone(), self.sender);
        move |event: &gpui::MouseDownEvent, window, cx| {
            act(Act::MenuFor(sender, event.position), window, cx)
        }
    }
}
