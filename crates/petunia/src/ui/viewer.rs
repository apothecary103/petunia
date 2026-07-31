//! A picture at full size, over everything else.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{Context, MouseButton, ScrollWheelEvent, SharedString, Window, div, px};
use gpui_component::IconName;

use super::{image, kit};
use petunia_config::Theme;
use crate::theme::ActivePalette;
use petunia_media::{audio, video};

/// Raised when the viewer wants to go away, so the workspace can drop it rather
/// than keep a hidden one around.
pub struct Dismissed;

impl gpui::EventEmitter<Dismissed> for Viewer {}

/// How far a picture can be blown up, and how far it can be shrunk. Beyond the
/// first there is nothing to see; below the second it is a thumbnail with a
/// black screen around it.
const ZOOM: std::ops::RangeInclusive<f32> = 0.2..=8.0;

pub struct Viewer {
    /// Everything of this kind in the thread, so left and right walk it.
    reel: Vec<PathBuf>,
    at: usize,
    zoom: f32,
    /// Where the picture has been dragged to, in logical pixels from centred.
    pan: gpui::Point<f32>,
    dragging: Option<gpui::Point<gpui::Pixels>>,
    /// Present only while the picture on screen is a video, so nothing decodes
    /// in the background.
    playing: Option<video::Player>,
    focus: gpui::FocusHandle,
}

impl Viewer {
    pub fn new(reel: Vec<PathBuf>, showing: &PathBuf, cx: &mut Context<Self>) -> Self {
        let at = reel.iter().position(|path| path == showing).unwrap_or(0);

        let mut viewer = Self {
            reel,
            at,
            zoom: 1.0,
            pan: gpui::point(0.0, 0.0),
            dragging: None,
            playing: None,
            focus: cx.focus_handle(),
        };
        viewer.open_video();
        viewer
    }

    /// A video gets a player; anything else does not, and the previous one is
    /// dropped so it stops decoding the moment it leaves the screen. Opening one
    /// starts it: you asked to watch it.
    fn open_video(&mut self) {
        self.playing = self
            .showing()
            .filter(|path| video::is_video(path))
            .and_then(|path| video::Player::open(path));
        if let Some(player) = self.playing.as_ref() {
            player.play();
        }
    }

    pub fn showing(&self) -> Option<&PathBuf> {
        self.reel.get(self.at)
    }

    /// Moving to another picture resets the view: carrying a zoom across would
    /// land the next one off screen.
    fn step(&mut self, by: isize, cx: &mut Context<Self>) {
        if self.reel.len() < 2 {
            return;
        }
        let count = self.reel.len() as isize;
        self.at = (((self.at as isize + by) % count + count) % count) as usize;
        self.reset(cx);
    }

    fn reset(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.pan = gpui::point(0.0, 0.0);
        self.open_video();
        cx.notify();
    }

    fn scale_by(&mut self, factor: f32, cx: &mut Context<Self>) {
        self.zoom = (self.zoom * factor).clamp(*ZOOM.start(), *ZOOM.end());
        if self.zoom <= 1.0 {
            self.pan = gpui::point(0.0, 0.0);
        }
        cx.notify();
    }

    fn copy(&self, cx: &mut Context<Self>) {
        let Some(path) = self.showing() else {
            return;
        };
        // The path rather than the pixels: gpui's clipboard carries text and
        // images, and a file reference is what another application can act on.
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(
            path.to_string_lossy().into_owned(),
        ));
    }

    fn save(&self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = self.showing().cloned() else {
            return;
        };
        let name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "image".into());
        let directory = dirs::download_dir().unwrap_or_else(std::env::temp_dir);
        let asked = cx.prompt_for_new_path(&directory, Some(&name));

        cx.spawn_in(window, async move |_, cx| {
            let Ok(Ok(Some(target))) = asked.await else {
                return;
            };
            let copied = cx
                .background_spawn(async move { std::fs::copy(&path, &target) })
                .await;
            if let Err(error) = copied {
                tracing::warn!(%error, "could not save the image");
            }
        })
        .detach();
    }

    fn open(&self) {
        let Some(path) = self.showing() else {
            return;
        };
        if let Err(error) = open::that_detached(path) {
            tracing::warn!(%error, "could not hand the file to the system");
        }
    }
}

impl gpui::Focusable for Viewer {
    fn focus_handle(&self, _cx: &gpui::App) -> gpui::FocusHandle {
        self.focus.clone()
    }
}

impl Render for Viewer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let Some(path) = self.showing().cloned() else {
            cx.emit(Dismissed);
            return div().into_any_element();
        };
        let position = format!("{} of {}", self.at + 1, self.reel.len());
        let zoom = self.zoom;
        let pan = self.pan;

        div()
            .id("viewer")
            .track_focus(&self.focus)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .flex_col()
            .bg(gpui::Hsla {
                a: 0.94,
                ..palette.background
            })
            .on_action(cx.listener(|_, _: &crate::actions::Cancel, _, cx| cx.emit(Dismissed)))
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = f32::from(event.delta.pixel_delta(px(20.0)).y);
                this.scale_by(1.0 + delta / 400.0, cx);
            }))
            .child(self.chrome(&position, &palette, cx))
            .child(
                div()
                    .id("stage")
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .overflow_hidden()
                    // Clicking the empty space around a picture closes it, which
                    // is what every viewer trains you to expect.
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
                    )
                    .child(
                        div()
                            .id("picture")
                            .relative()
                            .left(px(pan.x))
                            .top(px(pan.y))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, event: &gpui::MouseDownEvent, _, cx| {
                                    // Swallowed, so a click on the picture does
                                    // not reach the backdrop and close it.
                                    this.dragging = Some(event.position);
                                    cx.stop_propagation();
                                }),
                            )
                            .on_mouse_move(cx.listener(
                                |this, event: &gpui::MouseMoveEvent, _, cx| {
                                    let Some(from) = this.dragging else {
                                        return;
                                    };
                                    if !event.dragging() {
                                        this.dragging = None;
                                        return;
                                    }
                                    this.pan.x += f32::from(event.position.x - from.x);
                                    this.pan.y += f32::from(event.position.y - from.y);
                                    this.dragging = Some(event.position);
                                    cx.notify();
                                },
                            ))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.dragging = None),
                            )
                            // The viewer is the one place a picture is meant to
                            // be as large as it can be, so the resample target
                            // is the whole stage rather than a message's box.
                            .child(self.face(&path, zoom, window)),
                    ),
            )
            .when(self.playing.is_some(), |this| {
                this.child(self.transport(&palette, cx))
            })
            .when(self.reel.len() > 1, |this| {
                this.child(self.rail(&palette, cx))
            })
            .into_any_element()
    }
}

impl Viewer {
    /// A video draws its current frame into a platform surface; anything else is
    /// an ordinary picture. A frame that is not there yet leaves the stage dark
    /// rather than collapsing it, so nothing jumps when the first one arrives.
    fn face(&mut self, path: &std::path::Path, zoom: f32, window: &mut Window) -> gpui::AnyElement {
        #[cfg(target_os = "macos")]
        if let Some(player) = self.playing.as_mut() {
            // Unconditionally, not only while playing. AVFoundation loads the
            // asset on the run loop, so an item that is not ready yet reports a
            // rate of zero -- and gating the repaint on that is a deadlock: no
            // frame, so no repaint, so it never becomes ready.
            if !player.finished() {
                window.request_animation_frame();
            }

            // Its own shape, not a guess. `presentationSize` is zero until the
            // item has loaded, which is what the fallback box is holding.
            let (width, height) = match player.size() {
                Some(size) => super::message::media::fit(
                    Some(petunia_data::attachment::Size {
                        width: size.0 as u32,
                        height: size.1 as u32,
                    }),
                    (1100.0 * zoom, 700.0 * zoom),
                ),
                None => (900.0 * zoom, 560.0 * zoom),
            };

            return match player.frame() {
                Some(frame) => gpui::surface(frame)
                    .w(px(width))
                    .h(px(height))
                    .into_any_element(),
                None => div().w(px(width)).h(px(height)).into_any_element(),
            };
        }

        let _ = window;
        // A video with no player is one the system has no decoder for. Saying so
        // beats a blank stage, and the hand-off is the only thing left to offer.
        if video::is_video(path) {
            return div()
                .flex()
                .flex_col()
                .items_center()
                .gap_2()
                .child("This video cannot be played here.")
                .child(
                    div()
                        .id("hand-off")
                        .cursor_pointer()
                        .underline()
                        .on_mouse_down(
                            MouseButton::Left,
                            {
                                let path = path.to_path_buf();
                                move |_, _, _| {
                                    if let Err(error) = open::that_detached(&path) {
                                        tracing::warn!(%error, "could not hand the video over");
                                    }
                                }
                            },
                        )
                        .child("Open it with the system player"),
                )
                .into_any_element();
        }

        image::picture(path, 1600.0 * zoom, 1200.0 * zoom).into_any_element()
    }

    /// Play, a bar that scrubs, and the clock. Only drawn when there is a video
    /// to drive, so it is never a control that does nothing.
    fn transport(&self, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(player) = self.playing.as_ref() else {
            return div();
        };
        let playing = player.is_playing() && !player.finished();
        let position = player.position();
        let duration = player.duration();
        let fraction = duration
            .map(|duration| {
                (position.as_secs_f32() / duration.as_secs_f32().max(0.001)).clamp(0.0, 1.0)
            })
            .unwrap_or(0.0);

        let bounds: std::rc::Rc<std::cell::Cell<gpui::Bounds<gpui::Pixels>>> =
            std::rc::Rc::new(std::cell::Cell::new(gpui::Bounds::default()));
        let measured = bounds.clone();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_3()
            .px_4()
            .py_2p5()
            .child(
                div()
                    .id("video-play")
                    .size(px(32.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(palette.accent)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if let Some(player) = this.playing.as_ref() {
                                player.toggle();
                            }
                            cx.notify();
                        }),
                    )
                    .child(kit::icon(
                        if playing {
                            IconName::Pause
                        } else {
                            IconName::Play
                        },
                        14.0,
                        palette.on_accent,
                    )),
            )
            .child(
                div()
                    .id("video-scrub")
                    .relative()
                    .flex_1()
                    .min_w_0()
                    .h(px(18.0))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        gpui::canvas(move |at, _, _| measured.set(at), |_, _: (), _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, _, cx| {
                            let at = bounds.get();
                            if at.size.width <= px(0.0) {
                                return;
                            }
                            let fraction =
                                ((event.position.x - at.origin.x) / at.size.width).clamp(0.0, 1.0);
                            if let Some(player) = this.playing.as_ref() {
                                player.seek(fraction);
                            }
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w_full()
                            .h(px(4.0))
                            .rounded_full()
                            .bg(palette.border_focus)
                            .child(
                                div()
                                    .w(gpui::relative(fraction))
                                    .h_full()
                                    .rounded_full()
                                    .bg(palette.accent),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_none()
                    .text_size(px(palette.typography.ui_size - 2.0))
                    .text_color(palette.text_muted)
                    .child(SharedString::from(match duration {
                        Some(duration) => format!(
                            "{} / {}",
                            audio::clock(position),
                            audio::clock(duration)
                        ),
                        None => audio::clock(position),
                    })),
            )
    }

    fn chrome(
        &self,
        position: &str,
        palette: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let name = self
            .showing()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();

        div()
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_3()
            .h(px(super::workspace::TITLE_BAR))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text_dim)
                    .child(SharedString::from(name)),
            )
            .when(self.reel.len() > 1, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(SharedString::from(position.to_owned())),
                )
            })
            .child(kit::icon_button(
                "zoom-out",
                IconName::Minus,
                palette,
                cx.listener(|this, _, _, cx| this.scale_by(1.0 / 1.25, cx)),
            ))
            .child(kit::icon_button(
                "zoom-in",
                IconName::Plus,
                palette,
                cx.listener(|this, _, _, cx| this.scale_by(1.25, cx)),
            ))
            .child(kit::icon_button(
                "zoom-reset",
                IconName::Minimize,
                palette,
                cx.listener(|this, _, _, cx| this.reset(cx)),
            ))
            .child(kit::icon_button(
                "copy-image",
                IconName::Copy,
                palette,
                cx.listener(|this, _, _, cx| this.copy(cx)),
            ))
            .child(kit::icon_button(
                "save-image",
                IconName::ArrowDown,
                palette,
                cx.listener(|this, _, window, cx| this.save(window, cx)),
            ))
            .child(kit::icon_button(
                "open-image",
                IconName::ExternalLink,
                palette,
                cx.listener(|this, _, _, _| this.open()),
            ))
            .child(kit::icon_button(
                "close-viewer",
                IconName::Close,
                palette,
                cx.listener(|_, _, _, cx| cx.emit(Dismissed)),
            ))
    }

    /// Everything else in the thread, so moving between pictures is a click
    /// rather than a close and a hunt.
    fn rail(&self, palette: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let at = self.at;

        div()
            .id("reel")
            .flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .overflow_x_scroll()
            .child(kit::icon_button(
                "previous",
                IconName::ChevronLeft,
                palette,
                cx.listener(|this, _, _, cx| this.step(-1, cx)),
            ))
            .children(self.reel.iter().enumerate().map(|(index, path)| {
                div()
                    .id(SharedString::from(format!("reel-{index}")))
                    .flex_none()
                    .p_px()
                    .rounded(px(kit::RADIUS))
                    .cursor_pointer()
                    .border_1()
                    .border_color(if index == at {
                        palette.accent
                    } else {
                        gpui::transparent_black()
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.at = index;
                            this.reset(cx);
                        }),
                    )
                    .child(image::cropped(path, 44.0).rounded(px(kit::RADIUS - 1.0)))
            }))
            .child(kit::icon_button(
                "next",
                IconName::ChevronRight,
                palette,
                cx.listener(|this, _, _, cx| this.step(1, cx)),
            ))
    }
}

#[cfg(test)]
mod tests {
    /// Stepping wraps at both ends, which is the arithmetic worth pinning: a
    /// negative modulo in Rust is negative, and indexing with it panics.
    #[test]
    fn stepping_wraps_at_both_ends() {
        let step = |at: usize, by: isize, count: usize| {
            let count = count as isize;
            (((at as isize + by) % count + count) % count) as usize
        };

        assert_eq!(step(0, -1, 3), 2);
        assert_eq!(step(2, 1, 3), 0);
        assert_eq!(step(1, 1, 3), 2);
        assert_eq!(step(0, -5, 3), 1);
    }
}
