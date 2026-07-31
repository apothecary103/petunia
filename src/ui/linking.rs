use gpui::prelude::*;
use gpui::{Context, Entity, Window, div, px, white};
use gpui_component::ActiveTheme;
use qrcode::QrCode;

use crate::store::Store;

/// The one screen shown before an account exists: a QR code to scan with the
/// phone that already holds the account.
pub struct Linking {
    store: Entity<Store>,
}

impl Linking {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }
}

impl Render for Linking {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let store = self.store.read(cx);
        let theme = cx.theme();

        let body = match (&store.link_failure, &store.link_url) {
            (Some(failure), _) => div()
                .flex().flex_col()
                .gap_2()
                .items_center()
                .child(
                    div()
                        .text_color(theme.danger)
                        .child("Could not link this device."),
                )
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .text_sm()
                        .child(failure.clone()),
                )
                .child(
                    div()
                        .text_color(theme.muted_foreground)
                        .text_sm()
                        .child("Restart petunia to try again."),
                )
                .into_any_element(),
            (None, Some(url)) => div()
                .flex().flex_col()
                .gap_6()
                .items_center()
                .child(qr(url))
                .child(
                    div()
                        .flex().flex_col()
                        .gap_1()
                        .items_center()
                        .child(div().child("Scan this with Signal on your phone"))
                        .child(
                            div()
                                .text_color(theme.muted_foreground)
                                .text_sm()
                                .child("Settings → Linked devices → Link new device"),
                        ),
                )
                .into_any_element(),
            (None, None) => div()
                .text_color(theme.muted_foreground)
                .child("Connecting to Signal…")
                .into_any_element(),
        };

        div()
            .size_full()
            .flex().flex_col()
            .items_center()
            .justify_center()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(body)
    }
}

/// gpui has no QR widget, and the matrix is small enough that a grid of divs is
/// cheaper than decoding a generated image.
fn qr(url: &str) -> impl IntoElement {
    const MODULE: f32 = 5.0;
    const QUIET: f32 = 4.0 * MODULE;

    let Ok(code) = QrCode::new(url.as_bytes()) else {
        return div().child("Could not render the linking code.");
    };

    let width = code.width();
    let colors = code.to_colors();

    let rows = colors.chunks(width).map(|row| {
        div().flex().children(row.iter().map(|module| {
            div()
                .w(px(MODULE))
                .h(px(MODULE))
                .when(*module == qrcode::Color::Dark, |this| {
                    this.bg(gpui::black())
                })
        }))
    });

    div()
        .bg(white())
        .p(px(QUIET))
        .rounded(px(8.0))
        .child(div().flex().flex_col().children(rows))
}
