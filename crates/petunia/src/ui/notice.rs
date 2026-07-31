//! What went wrong, said once and out of the way.
//!
//! A stack rather than a single slot: two failures a second apart used to mean
//! the first one was never seen.

use std::time::{Duration, Instant};

use gpui::prelude::*;
use gpui::{Context, SharedString, Window, div, px};
use gpui_component::IconName;

use super::kit;
use petunia_config::Theme;
use crate::theme::ActivePalette;

/// How long a notice stays up before it goes on its own.
const LINGER: Duration = Duration::from_secs(6);

/// At most this many at once; older ones drop off the bottom rather than
/// covering the conversation.
const MAX: usize = 3;

struct Card {
    text: SharedString,
    raised: Instant,
}

#[derive(Default)]
pub struct Notices {
    cards: Vec<Card>,
    /// Runs only while something is waiting to expire.
    sweeping: Option<gpui::Task<()>>,
}

impl Notices {
    pub fn raise(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        let text = text.into();
        // The same thing failing twice is one notice, not two: a reconnect loop
        // would otherwise fill the screen with the same sentence.
        if self.cards.last().is_some_and(|card| card.text == text) {
            return;
        }

        self.cards.push(Card {
            text,
            raised: Instant::now(),
        });
        while self.cards.len() > MAX {
            self.cards.remove(0);
        }
        self.sweep(cx);
        cx.notify();
    }

    /// Everything raised so far is a failure, and a failure gets long enough to
    /// be read rather than the two seconds a confirmation would want.
    fn sweep(&mut self, cx: &mut Context<Self>) {
        if self.sweeping.is_some() {
            return;
        }
        self.sweeping = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(LINGER / 3).await;
                let carry_on = this.update(cx, |this: &mut Self, cx| {
                    let before = this.cards.len();
                    this.cards.retain(|card| card.raised.elapsed() < LINGER);
                    if this.cards.len() != before {
                        cx.notify();
                    }
                    !this.cards.is_empty()
                });
                match carry_on {
                    Ok(true) => {}
                    _ => break,
                }
            }
            this.update(cx, |this, _| this.sweeping = None).ok();
        }));
    }
}

impl Render for Notices {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();

        div()
            .absolute()
            .bottom_4()
            .right_4()
            .flex()
            .flex_col()
            .gap_2()
            .children(
                self.cards
                    .iter()
                    .enumerate()
                    .map(|(index, card)| self.card(index, card, &palette, cx)),
            )
    }
}

impl Notices {
    fn card(
        &self,
        index: usize,
        card: &Card,
        palette: &Theme,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(format!("notice-{index}")))
            .flex()
            .items_start()
            .gap_2p5()
            .max_w(px(360.0))
            .px_3()
            .py_2p5()
            .rounded(px(kit::RADIUS))
            .bg(palette.elevated)
            .border_1()
            .border_color(palette.border)
            .child(kit::icon(IconName::TriangleAlert, 15.0, palette.danger))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_dim)
                    .child(card.text.clone()),
            )
            .child(kit::icon_button(
                SharedString::from(format!("dismiss-{index}")),
                IconName::Close,
                palette,
                cx.listener(move |this: &mut Self, _, _, cx| {
                    if index < this.cards.len() {
                        this.cards.remove(index);
                        cx.notify();
                    }
                }),
            ))
    }
}
