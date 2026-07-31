//! What an attachment looks like in the conversation.

use std::path::Path;

use gpui::prelude::*;
use gpui::{AnyElement, Div, MouseButton, SharedString, div, px};
use gpui_component::progress::Progress;
use gpui_component::{IconName, Sizable};

use super::act::{Act, Dispatch};
use crate::audio::{self, Playback};
use crate::config::Theme;
use crate::config::messages::Spacing;
use crate::data::attachment::{Attachment, Blob, Kind, Size};
use crate::ui::{image, kit};

/// Everything the renderer needs that is not the attachment itself.
pub struct Frame<'a> {
    pub theme: &'a Theme,
    pub spacing: Spacing,
    /// The box inline media is scaled to fit inside.
    pub max_image: (f32, f32),
    /// The message the attachment hangs off, which is how a download names what
    /// it wants.
    pub timestamp: u64,
    pub playback: &'a Playback,
    pub act: &'a Dispatch,
}

impl Frame<'_> {
    pub fn render(&self, attached: &Attachment) -> AnyElement {
        match (&attached.kind, &attached.blob) {
            (Kind::Image { size, .. }, Blob::Cached(path)) => self.picture(attached, *size, path),
            (Kind::Video { size, duration }, Blob::Cached(path)) => {
                self.video(*size, *duration, path)
            }
            (Kind::Audio { waveform, .. }, Blob::Cached(path)) => {
                self.audio(attached, waveform.as_deref(), path)
            }
            (_, Blob::Cached(path)) => self.file(attached, path),
            (_, Blob::Downloading) => self.downloading(attached),
            (_, Blob::Failed(error)) => self.failed(attached, error),
            (_, Blob::Missing) => self.missing(attached),
        }
    }

    /// Sized explicitly rather than capped: an image's natural size is its pixel
    /// size, and a maximum alone leaves the layout to guess the other axis.
    fn picture(&self, attached: &Attachment, size: Option<Size>, path: &Path) -> AnyElement {
        let (width, height) = fit(size, self.max_image);
        let act = self.act.clone();
        let target = path.to_path_buf();

        div()
            .id(SharedString::from(format!("view-{}", attached.id.as_str())))
            .flex_none()
            .w(px(width))
            .h(px(height))
            .cursor_pointer()
            .rounded(px(kit::RADIUS))
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                act(Act::View(target.clone()), window, cx)
            })
            .child(image::picture(path, width, height))
            .into_any_element()
    }

    /// A poster frame with the controls over it. What plays it is the viewer,
    /// which is where a video has room to be watched.
    fn video(
        &self,
        size: Option<Size>,
        duration: Option<std::time::Duration>,
        path: &Path,
    ) -> AnyElement {
        let theme = self.theme;
        let (width, height) = fit(size, self.max_image);
        let act = self.act.clone();
        let target = path.to_path_buf();

        div()
            .id(SharedString::from(format!("play-{}", stem(path))))
            .relative()
            .flex_none()
            .w(px(width))
            .h(px(height))
            .cursor_pointer()
            .rounded(px(kit::RADIUS))
            .overflow_hidden()
            .bg(theme.sunken)
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                act(Act::View(target.clone()), window, cx)
            })
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        div()
                            .size(px(48.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_full()
                            .bg(kit::tinted(theme.text))
                            .child(kit::icon(IconName::Play, 20.0, theme.text)),
                    ),
            )
            .when_some(duration, |this, duration| {
                this.child(
                    div()
                        .absolute()
                        .bottom_1p5()
                        .right_1p5()
                        .px_1p5()
                        .rounded(px(4.0))
                        .bg(theme.background)
                        .text_size(px(self.spacing.small))
                        .text_color(theme.text_dim)
                        .child(SharedString::from(audio::clock(duration))),
                )
            })
            .into_any_element()
    }

    /// A voice note as Signal draws one: a play control, the shape of the sound,
    /// and how long it runs.
    fn audio(&self, attached: &Attachment, waveform: Option<&[u8]>, path: &Path) -> AnyElement {
        let theme = self.theme;
        let mine = self.playback.is(path);
        let playing = mine && self.playback.playing;
        let progress = if mine {
            self.playback.fraction().unwrap_or(0.0)
        } else {
            0.0
        };
        let elapsed = match (mine, length(attached)) {
            (true, _) => Some(self.playback.position),
            (false, length) => length,
        };

        let act = self.act.clone();
        let target = path.to_path_buf();
        let seek = self.act.clone();
        let seek_target = path.to_path_buf();

        chip_shell(theme)
            .w(px(280.0))
            .child(
                div()
                    .id(SharedString::from(format!("audio-{}", attached.id.as_str())))
                    .size(px(30.0))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(theme.accent)
                    .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                        act(Act::Play(target.clone()), window, cx)
                    })
                    .child(kit::icon(
                        if playing {
                            IconName::Pause
                        } else {
                            IconName::Play
                        },
                        14.0,
                        theme.on_accent,
                    )),
            )
            .child(
                waveform_strip(attached, waveform, progress, theme, seek, seek_target),
            )
            .when_some(elapsed, |this, elapsed| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(self.spacing.small))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(audio::clock(elapsed))),
                )
            })
            .into_any_element()
    }

    fn file(&self, attached: &Attachment, path: &Path) -> AnyElement {
        let theme = self.theme;
        let act = self.act.clone();
        let target = path.to_path_buf();
        let save = self.act.clone();
        let saved = path.to_path_buf();

        chip(attached, size(attached.size), theme.text_muted, theme)
            .id(SharedString::from(format!("open-{}", attached.id.as_str())))
            .cursor_pointer()
            .hover(|this| this.border_color(theme.border_focus))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                act(Act::Open(target.clone()), window, cx)
            })
            .child(kit::icon_button(
                SharedString::from(format!("save-{}", attached.id.as_str())),
                IconName::ArrowDown,
                theme,
                move |_, window, cx| save(Act::Save(saved.clone()), window, cx),
            ))
            .into_any_element()
    }

    /// The bar slides rather than fills: presage hands back the whole file at
    /// once, so the only honest thing to show is that something is happening.
    fn downloading(&self, attached: &Attachment) -> AnyElement {
        let theme = self.theme;

        chip_shell(theme)
            .child(kit::icon(icon_for(&attached.kind), 16.0, theme.text_muted))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        div()
                            .truncate()
                            .text_size(px(theme.typography.ui_size))
                            .text_color(theme.text)
                            .child(SharedString::from(label(attached))),
                    )
                    .child(
                        Progress::new(SharedString::from(format!(
                            "downloading-{}",
                            attached.id.as_str()
                        )))
                        .loading(true)
                        .color(theme.accent)
                        .with_size(gpui_component::Size::XSmall),
                    )
                    .child(
                        div()
                            .text_size(px(theme.typography.ui_size - 3.0))
                            .text_color(theme.text_muted)
                            .child(SharedString::from(format!(
                                "Downloading… {}",
                                size(attached.size)
                            ))),
                    ),
            )
            .into_any_element()
    }

    fn failed(&self, attached: &Attachment, error: &str) -> AnyElement {
        let theme = self.theme;
        self.retryable(
            chip(
                attached,
                format!("Could not download — {error}"),
                theme.danger,
                theme,
            ),
            attached,
        )
    }

    fn missing(&self, attached: &Attachment) -> AnyElement {
        let theme = self.theme;
        self.retryable(
            chip(
                attached,
                format!("{} · tap to download", size(attached.size)),
                theme.text_muted,
                theme,
            ),
            attached,
        )
    }

    fn retryable(&self, chip: gpui::Stateful<Div>, attached: &Attachment) -> AnyElement {
        let theme = self.theme;
        let act = self.act.clone();
        let id = attached.id.clone();
        let timestamp = self.timestamp;

        chip.cursor_pointer()
            .hover(|this| this.border_color(theme.border_focus))
            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                act(
                    Act::Download {
                        timestamp,
                        id: id.clone(),
                    },
                    window,
                    cx,
                )
            })
            .into_any_element()
    }
}

/// Clicking the waveform seeks to that point, which means turning a click into a
/// fraction — and nothing in a layout closure knows how wide it ended up. A
/// `canvas` behind the bars records the bounds it was laid out at, which the
/// click handler then reads.
fn waveform_strip(
    attached: &Attachment,
    waveform: Option<&[u8]>,
    progress: f32,
    theme: &Theme,
    act: Dispatch,
    path: std::path::PathBuf,
) -> gpui::Stateful<Div> {
    use std::cell::Cell;
    use std::rc::Rc;

    let bounds: Rc<Cell<gpui::Bounds<gpui::Pixels>>> = Rc::new(Cell::new(gpui::Bounds::default()));
    let measured = bounds.clone();

    div()
        .id(SharedString::from(format!("seek-{}", attached.id.as_str())))
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .gap_px()
        .h(px(26.0))
        .cursor_pointer()
        .child(
            gpui::canvas(
                move |at, _, _| measured.set(at),
                |_, _: (), _, _| {},
            )
            .absolute()
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                let at = bounds.get();
                if at.size.width <= gpui::px(0.0) {
                    return;
                }
                let fraction = ((event.position.x - at.origin.x) / at.size.width).clamp(0.0, 1.0);
                act(Act::Seek(path.clone(), fraction), window, cx)
            },
        )
        .children(bars(waveform, progress, theme))
}

/// Signal ships a waveform with a voice note, so the shape of the sound is drawn
/// from the protocol rather than by decoding anything.
fn bars(waveform: Option<&[u8]>, progress: f32, theme: &Theme) -> Vec<AnyElement> {
    const COUNT: usize = 44;

    audio::bars(waveform, COUNT)
        .into_iter()
        .enumerate()
        .map(|(index, height)| {
            let played = (index as f32 / COUNT as f32) < progress;
            div()
                .flex_1()
                .mx_px()
                .h(px(4.0 + height * 18.0))
                .rounded_full()
                .bg(if played { theme.accent } else { theme.border_focus })
                .into_any_element()
        })
        .collect()
}

fn length(attached: &Attachment) -> Option<std::time::Duration> {
    match attached.kind {
        Kind::Audio { duration, .. } => duration,
        _ => None,
    }
}

/// Scales an image down to fit inside the box, keeping its aspect ratio and
/// never scaling up. Without the pixel size to work from, the box is all there
/// is to honour.
pub fn fit(size: Option<Size>, max: (f32, f32)) -> (f32, f32) {
    let Some(size) = size.filter(|size| size.width > 0 && size.height > 0) else {
        return max;
    };

    let (width, height) = (size.width as f32, size.height as f32);
    let scale = (max.0 / width).min(max.1 / height).min(1.0);

    (width * scale, height * scale)
}

pub fn icon_for(kind: &Kind) -> IconName {
    match kind {
        Kind::Image { .. } => IconName::Frame,
        Kind::Video { .. } => IconName::Play,
        Kind::Audio { .. } => IconName::Bell,
        Kind::File => IconName::File,
    }
}

pub fn label(attached: &Attachment) -> String {
    attached
        .file_name
        .clone()
        .unwrap_or_else(|| match attached.kind {
            Kind::Image { .. } => "Photo".into(),
            Kind::Video { .. } => "Video".into(),
            Kind::Audio {
                voice_note: true, ..
            } => "Voice message".into(),
            Kind::Audio { .. } => "Audio".into(),
            Kind::File => "File".into(),
        })
}

/// Bytes as something a person reads, not as a number of bytes.
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn chip_shell(theme: &Theme) -> Div {
    div()
        .flex()
        .items_center()
        .gap_2p5()
        .px_3()
        .py_2()
        .rounded(px(kit::RADIUS))
        .bg(theme.elevated)
        .border_1()
        .border_color(theme.border)
}

fn chip(
    attached: &Attachment,
    detail: String,
    tint: gpui::Hsla,
    theme: &Theme,
) -> gpui::Stateful<Div> {
    chip_shell(theme)
        .id(SharedString::from(format!("chip-{}", attached.id.as_str())))
        .child(kit::icon(icon_for(&attached.kind), 16.0, tint))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme.typography.ui_size))
                        .text_color(theme.text)
                        .child(SharedString::from(label(attached))),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(theme.typography.ui_size - 3.0))
                        .text_color(tint)
                        .child(SharedString::from(detail)),
                ),
        )
}

fn stem(path: &Path) -> String {
    path.file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(left: (f32, f32), right: (f32, f32)) {
        assert!(
            (left.0 - right.0).abs() < 0.01 && (left.1 - right.1).abs() < 0.01,
            "{left:?} is not {right:?}"
        );
    }

    #[test]
    fn a_wide_image_is_bounded_by_the_width() {
        close(
            fit(
                Some(Size {
                    width: 3000,
                    height: 1500,
                }),
                (400.0, 300.0),
            ),
            (400.0, 200.0),
        );
    }

    #[test]
    fn a_tall_image_is_bounded_by_the_height() {
        close(
            fit(
                Some(Size {
                    width: 1000,
                    height: 4000,
                }),
                (400.0, 300.0),
            ),
            (75.0, 300.0),
        );
    }

    /// Enlarging a small picture to fill the box makes it worse, not bigger.
    #[test]
    fn a_small_image_keeps_its_own_size() {
        let (width, height) = fit(
            Some(Size {
                width: 80,
                height: 60,
            }),
            (400.0, 300.0),
        );

        assert_eq!((width, height), (80.0, 60.0));
    }

    /// Zero is what the protocol sends when it does not know, and dividing by it
    /// puts an image nowhere.
    #[test]
    fn an_unknown_size_falls_back_to_the_box() {
        assert_eq!(fit(None, (400.0, 300.0)), (400.0, 300.0));
        assert_eq!(
            fit(
                Some(Size {
                    width: 0,
                    height: 100
                }),
                (400.0, 300.0)
            ),
            (400.0, 300.0)
        );
    }

    #[test]
    fn sizes_read_as_units() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(2048), "2.0 KB");
        assert_eq!(size(5 * 1024 * 1024), "5.0 MB");
    }
}
