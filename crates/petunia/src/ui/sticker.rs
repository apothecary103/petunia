//! A sticker, and the pack it came from.
//!
//! Clicking a sticker used to install its pack outright, with a tooltip for a
//! warning: one click, no confirmation, and nothing to look at first. That is
//! not what the gesture means anywhere else — on the phone a sticker opens the
//! pack it belongs to, shows the rest of it, and offers to add it. So this is
//! that: the sticker at a size worth looking at, what the pack is called, who
//! made it, every other sticker in it, and one button.
//!
//! A pack this account does not have is *read* rather than installed. The
//! manifest lives behind the pack key and fetching it is the only way to know
//! what a pack is called, which presage would only do as part of installing —
//! so `preview_sticker_pack` is ours, and the difference it makes is that a
//! sticker from a pack you have never seen opens the same sheet as one from
//! your own, rather than an empty box asking you to commit first.

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Window, div, px};

use super::{image, kit};
use crate::store::{Previewed, Store};
use crate::theme::ActivePalette;
use petunia_config::Theme;
use petunia_data::attachment::Blob;
use petunia_data::message::Sticker;
use petunia_data::stickers::Pack;
use petunia_signal::Command;

pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for StickerSheet {}

/// How large the sticker itself is drawn. Signal's own preview is about this:
/// large enough to be the subject of the sheet rather than an illustration of
/// it, small enough that the pack below it is still on screen.
const FACE: f32 = 176.0;

/// How large one of the pack's other stickers is.
const TILE: f32 = 64.0;

pub struct StickerSheet {
    sticker: Sticker,
    store: Entity<Store>,
    focus: gpui::FocusHandle,
}

impl StickerSheet {
    pub fn new(sticker: Sticker, store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        // The pack arrives as an event, whether it was read or installed, so the
        // sheet has to redraw when the store does or it would sit on whatever it
        // knew the moment it opened.
        cx.observe(&store, |_, _, cx| cx.notify()).detach();

        let sheet = Self {
            sticker,
            store,
            focus: cx.focus_handle(),
        };
        sheet.read_pack(cx);
        sheet
    }

    pub fn take_focus(&self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    /// Asks for the pack behind this sticker when it is one this account does
    /// not have. Nothing is installed by it and no other device is told.
    fn read_pack(&self, cx: &mut Context<Self>) {
        let Some(key) = self.sticker.pack_key.clone() else {
            return;
        };
        let pack_id = self.sticker.pack_id.clone();
        self.store.update(cx, |store, _| {
            if store.installed(&pack_id).is_none() {
                store.preview_pack(pack_id, key);
            }
        });
    }

    /// What is known about the pack this sticker belongs to.
    fn pack<'a>(&self, store: &'a Store) -> Known<'a> {
        if let Some(pack) = store.installed(&self.sticker.pack_id) {
            return Known::Installed(pack);
        }
        match store.preview(&self.sticker.pack_id) {
            Some(Previewed::Read(pack)) => Known::Read(pack),
            Some(Previewed::Failed(why)) => Known::Failed(why),
            Some(Previewed::Reading) => Known::Reading,
            // Nothing was asked for, which is a sticker that arrived without its
            // key -- the one case there is no way to read the pack at all.
            None => Known::Nothing,
        }
    }

    fn install(&mut self, cx: &mut Context<Self>) {
        let Some(key) = self.sticker.pack_key.clone() else {
            return;
        };
        let pack_id = self.sticker.pack_id.clone();
        self.store.update(cx, |store, _| {
            store.send(Command::InstallStickerPack { pack_id, key })
        });
        cx.notify();
    }
}

/// What is known about the pack behind the sticker on the sheet.
enum Known<'a> {
    Installed(&'a Pack),
    /// Read from the server but not added, which is what a sticker from a pack
    /// this account does not have opens as.
    Read(&'a Pack),
    Reading,
    Failed(&'a str),
    Nothing,
}

impl Known<'_> {
    fn pack(&self) -> Option<&Pack> {
        match self {
            Self::Installed(pack) | Self::Read(pack) => Some(pack),
            _ => None,
        }
    }
}

impl gpui::Focusable for StickerSheet {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for StickerSheet {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let known = self.pack(self.store.read(cx));
        let installed = matches!(known, Known::Installed(_));
        let installable = !installed && self.sticker.pack_key.is_some();
        let pack = known.pack();

        let title = match pack {
            Some(pack) => pack.title.clone(),
            None => "Sticker".to_owned(),
        };
        let author = pack
            .map(|pack| pack.author.clone())
            .filter(|author| !author.is_empty());
        let said = note(&known, &palette);
        let contents = pack.map(|pack| rest_of(pack, self.sticker.sticker_id, &palette));

        kit::scrim(&palette)
            .id("sticker-sheet")
            .track_focus(&self.focus)
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            )
            .child(
                kit::dialog(360.0, &palette)
                    .child(self.face(&palette))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size + 1.0))
                                    .text_color(palette.text)
                                    .child(SharedString::from(title)),
                            )
                            .children(author.map(|author| {
                                div()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size - 1.0))
                                    .text_color(palette.text_muted)
                                    .child(SharedString::from(format!("by {author}")))
                            }))
                            .child(said),
                    )
                    .children(contents)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap_2()
                            .child(kit::button(
                                "close-sticker",
                                "Close",
                                kit::Intent::Quiet,
                                &palette,
                                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                            ))
                            // No button at all for a pack that is already here,
                            // and none for a sticker that arrived without the
                            // key: neither has anything left to offer, and a
                            // control that says "Added" is a control that lies
                            // about being one.
                            .children(installable.then(|| {
                                kit::button(
                                    "add-pack",
                                    "Add pack",
                                    kit::Intent::Primary,
                                    &palette,
                                    cx.listener(|this: &mut Self, _, _, cx| this.install(cx)),
                                )
                            })),
                    ),
            )
    }
}

/// The line under the name that says what this sheet is looking at: a pack you
/// have, one being read, one that would not read, or a sticker with no key
/// behind it. Every state says something, because a sheet that draws nothing
/// while it waits reads as one that is broken.
fn note(known: &Known<'_>, palette: &Theme) -> gpui::Div {
    let said = match known {
        Known::Installed(pack) => format!("{} · added", count(pack.stickers.len())),
        Known::Read(pack) => count(pack.stickers.len()),
        Known::Reading => "Reading the pack…".to_owned(),
        Known::Failed(why) => (*why).to_owned(),
        Known::Nothing => "This sticker arrived without its pack.".to_owned(),
    };
    let tint = match known {
        Known::Failed(_) => palette.danger,
        _ => palette.text_muted,
    };

    div()
        .pt_0p5()
        .text_size(px(palette.typography.ui_size - 2.0))
        .text_color(tint)
        .child(SharedString::from(said))
}

fn count(stickers: usize) -> String {
    match stickers {
        1 => "1 sticker".to_owned(),
        many => format!("{many} stickers"),
    }
}

impl StickerSheet {
    /// The sticker itself, on the sunken fill rather than the card's: a sticker
    /// is a cut-out with transparency around it, and on a surface the same
    /// colour as the card it floats with no edge at all.
    fn face(&self, palette: &Theme) -> gpui::Div {
        let square = div()
            .size(px(FACE))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .rounded(px(kit::RADIUS))
            .bg(palette.sunken);

        let square = match self.sticker.image.as_ref().map(|image| &image.blob) {
            Some(Blob::Cached(path)) => {
                square.child(image::animated("frames", path, FACE - 16.0, FACE - 16.0))
            }
            // The pack's own emoji holds the space, the same as it does in the
            // message: a sticker that will not decode must not collapse.
            _ => square.text_size(px(FACE * 0.4)).child(SharedString::from(
                self.sticker.emoji.clone().unwrap_or_else(|| "🎨".into()),
            )),
        };

        div().flex().justify_center().child(square)
    }
}

/// The whole pack, so the sheet answers "what else is in this" without a second
/// click. The one being looked at is marked rather than left out: a grid with a
/// sticker missing from it is a grid in an order that is not the pack's, and
/// "which of these am I looking at" then has no answer.
///
/// Clicking one is deliberately not wired to anything: sending from here would
/// be sending into whichever conversation happens to be behind the sheet, and
/// the picker in the composer is where that belongs.
fn rest_of(pack: &Pack, showing: u32, palette: &Theme) -> gpui::Stateful<gpui::Div> {
    div()
        .id("pack-contents")
        .max_h(px(180.0))
        .overflow_y_scroll()
        .child(
            div()
                .flex()
                .flex_wrap()
                .gap_1()
                .children(pack.stickers.iter().map(|sticker| {
                    div()
                        .size(px(TILE))
                        .flex()
                        .flex_none()
                        .items_center()
                        .justify_center()
                        .rounded(px(kit::RADIUS))
                        .bg(palette.sunken)
                        .when(sticker.id == showing, |this| {
                            this.border_1().border_color(palette.accent)
                        })
                        .child(image::picture(&sticker.path, TILE - 10.0, TILE - 10.0))
                })),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_sticker_is_not_one_stickers() {
        assert_eq!(count(1), "1 sticker");
        assert_eq!(count(24), "24 stickers");
    }
}
