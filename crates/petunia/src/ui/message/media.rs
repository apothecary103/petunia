//! What an attachment looks like in the conversation.

use std::path::Path;

use gpui::prelude::*;
use gpui::{AnyElement, Div, MouseButton, SharedString, div, px};
use gpui_component::progress::Progress;
use gpui_component::{IconName, Sizable};

use gpui_component::highlighter::HighlightTheme;

use super::act::{Act, Dispatch};
use super::{content, text};
use petunia_media::audio::{self, Playback};
use petunia_config::Theme;
use petunia_config::messages::Spacing;
use petunia_data::attachment::{Attachment, Blob, Kind, Size};
use crate::ui::{image, kit};

/// Everything the renderer needs that is not the attachment itself.
pub struct Frame<'a> {
    pub theme: &'a Theme,
    /// The colours a previewed text file is highlighted in. Derived once when
    /// the theme is installed, because this is rendered per frame.
    pub highlights: &'a HighlightTheme,
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
    /// An attachment with the caption its sender wrote under it, when there is
    /// one. A caption is not the message body and must not be folded into it.
    pub fn render(&self, attached: &Attachment) -> AnyElement {
        let media = self.body(attached);
        let Some(caption) = attached.caption.clone().filter(|text| !text.trim().is_empty())
        else {
            return media;
        };

        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(media)
            .child(
                div()
                    .max_w(px(self.max_image.0))
                    .text_size(px(self.spacing.small))
                    .text_color(self.theme.text_dim)
                    .child(SharedString::from(caption)),
            )
            .into_any_element()
    }

    fn body(&self, attached: &Attachment) -> AnyElement {
        match (&attached.kind, &attached.blob) {
            (Kind::Image { size, .. }, Blob::Cached(path)) => self.picture(attached, *size, path),
            (
                Kind::Video {
                    size,
                    duration,
                    poster,
                },
                Blob::Cached(path),
            ) => self.video(*size, *duration, poster.as_deref(), path),
            // A record is drawn as one and a voice note as one, and the sender
            // says which: `voice_note` is Signal's own mark, and a mark outranks
            // anything read out of the bytes.
            (
                Kind::Audio {
                    waveform,
                    voice_note,
                    ..
                },
                Blob::Cached(path),
            ) => match song(path).filter(|song| !voice_note && song.is_a_record()) {
                Some(song) => self.record(attached, &song, path),
                None => self.audio(attached, waveform.as_deref(), path),
            },
            (_, Blob::Cached(path)) => match text::language_of(attached.file_name.as_deref(), path)
                .zip(text::head(path))
            {
                Some((language, head)) => self.text(attached, path, language, &head),
                None => self.file(attached, path),
            },
            (_, Blob::Downloading(fraction)) => self.downloading(attached, *fraction),
            (_, Blob::Failed(error)) => self.failed(attached, error),
            (_, Blob::Missing) => self.missing(attached),
        }
    }

    /// Sized explicitly rather than capped: an image's natural size is its pixel
    /// size, and a maximum alone leaves the layout to guess the other axis.
    ///
    /// The shape comes from the file rather than from what the sender said it
    /// was, which is what a picture drawn with margin around it is: see
    /// `image::shape`.
    fn picture(&self, attached: &Attachment, size: Option<Size>, path: &Path) -> AnyElement {
        let (width, height) = fit(image::shape(path).or(size), self.max_image);
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
            // Given an id, because a GIF arrives here and gpui keeps which frame
            // it is showing in element state that only an id gets it.
            .child(
                image::animated("frames", path, width, height).rounded(px(kit::RADIUS)),
            )
            .into_any_element()
    }

    /// A poster frame with the controls over it. What plays it is the viewer,
    /// which is where a video has room to be watched.
    fn video(
        &self,
        size: Option<Size>,
        duration: Option<std::time::Duration>,
        poster: Option<&Path>,
        path: &Path,
    ) -> AnyElement {
        let theme = self.theme;
        // The poster is the frame actually on screen, and it was generated here
        // from the clip itself, so it is a better answer than the declaration --
        // which for video is missing more often than not.
        let (width, height) = fit(poster.and_then(image::shape).or(size), self.max_image);
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
            .when_some(poster, |this, poster| {
                this.child(image::picture(poster, width, height).rounded(px(kit::RADIUS)))
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

    /// A record as a record, laid out the way every Apple player lays one out:
    /// the artwork, the title and who made it beside it, and the transport across
    /// the bottom with the times *under* the bar at either end.
    ///
    /// The times are the whole of what the old shape got wrong. A single clock
    /// crammed in at the end of the row read as a duration when nothing was
    /// playing and as a position when something was, said nothing about how much
    /// was left, and pushed the bar into whatever width was left over — three
    /// controls fighting for one line. Elapsed on the left and what remains on
    /// the right, both under the bar they describe, is the arrangement in Music,
    /// in QuickTime and in every transport Apple ships, and it leaves the bar the
    /// full width of the card.
    ///
    /// The waveform is left off deliberately. Signal ships one with a voice note
    /// and nothing else, so on a track it is forty-four identical grey bars — a
    /// picture of no information, standing where the cover belongs.
    fn record(
        &self,
        attached: &Attachment,
        song: &petunia_media::song::Song,
        path: &Path,
    ) -> AnyElement {
        let theme = self.theme;
        let mine = self.playback.is(path);
        let playing = mine && self.playback.playing;
        let progress = if mine {
            self.playback.fraction().unwrap_or(0.0)
        } else {
            0.0
        };
        let whole = song.duration.or_else(|| length(attached));
        let position = match mine {
            true => self.playback.position,
            false => std::time::Duration::ZERO,
        };
        // What is left, which is the number a player is actually asked for. Signed
        // the way every transport signs it.
        let left = whole
            .map(|whole| whole.saturating_sub(position))
            .map(|left| format!("-{}", audio::clock(left)));

        let act = self.act.clone();
        let target = path.to_path_buf();
        let seek = self.act.clone();
        let seek_target = path.to_path_buf();

        // Who made it and what it is off, joined only when both are known: a
        // separator with nothing on one side of it is a separator reporting a
        // field that is missing.
        let credit = [song.artist.as_deref(), song.album.as_deref()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" — ");

        div()
            .flex()
            .flex_col()
            .gap_3()
            .w(px(340.0))
            .p_3()
            .rounded(px(kit::RADIUS_LG))
            .bg(theme.elevated)
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .size(px(COVER))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .overflow_hidden()
                            .rounded(px(kit::RADIUS))
                            .bg(theme.sunken)
                            // The hairline is Apple's: artwork is a photograph of
                            // an object, and a light cover on a light card has no
                            // edge of its own to end at.
                            .border_1()
                            .border_color(theme.border)
                            // Rounded on the picture as well as on the well behind
                            // it: `overflow_hidden` clips a child to the parent's
                            // rectangle rather than to its corners, so a square
                            // cover in a rounded box keeps its own square corners.
                            .when(song.cover, |this| {
                                this.child(image::artwork(path, COVER).rounded(px(kit::RADIUS)))
                            })
                            // A record with no artwork gets the mark a record
                            // gets, not the bell an attached sound gets.
                            .when(!song.cover, |this| {
                                this.child(kit::glyph("icons/music.svg", 20.0, theme.text_dim))
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap_0p5()
                            .child(
                                div()
                                    .truncate()
                                    .text_size(px(theme.typography.ui_size + 1.0))
                                    .font_weight(kit::EMPHASIS)
                                    .text_color(theme.text)
                                    .child(SharedString::from(
                                        song.title.clone().unwrap_or_else(|| label(attached)),
                                    )),
                            )
                            .when(!credit.is_empty(), |this| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(px(theme.typography.ui_size))
                                        .text_color(theme.text_dim)
                                        .child(SharedString::from(credit)),
                                )
                            })
                            .when_some(song.quality(), |this, quality| {
                                this.child(
                                    div()
                                        .truncate()
                                        .text_size(px(self.spacing.small))
                                        .text_color(theme.text_muted)
                                        .child(SharedString::from(quality)),
                                )
                            }),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(
                        div()
                            .id(SharedString::from(format!("track-{}", attached.id.as_str())))
                            .size(px(32.0))
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
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .child(rail(
                                format!("scrub-{}", attached.id.as_str()),
                                progress,
                                theme,
                                seek,
                                seek_target,
                            ))
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_between()
                                    .text_size(px(self.spacing.small))
                                    .text_color(theme.text_muted)
                                    .child(SharedString::from(audio::clock(position)))
                                    .children(left.map(SharedString::from)),
                            ),
                    )
                    .child(speed_pill(
                        format!("speed-{}", attached.id.as_str()),
                        self.playback.speed,
                        self.spacing.small,
                        theme,
                        self.act.clone(),
                    )),
            )
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
            .child(wave(
                format!("seek-{}", attached.id.as_str()),
                sound(waveform, path),
                progress,
                theme,
                seek,
                seek_target,
            ))
            .when_some(elapsed, |this, elapsed| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(self.spacing.small))
                        .text_color(theme.text_muted)
                        .child(SharedString::from(audio::clock(elapsed))),
                )
            })
            .child(speed_pill(
                format!("speed-{}", attached.id.as_str()),
                self.playback.speed,
                self.spacing.small,
                theme,
                self.act.clone(),
            ))
            .into_any_element()
    }

    /// A text file as its own first lines, the way it would read if it had been
    /// pasted into the message instead of attached to it -- which is why it is
    /// drawn in the very box a pasted listing gets, down to the strip across the
    /// top. What the strip carries is the difference between the two: a file has
    /// an icon, a name and a size where a listing has the language it is in, and
    /// the button on the right saves it rather than copying it.
    fn text(
        &self,
        attached: &Attachment,
        path: &Path,
        language: &str,
        head: &text::Head,
    ) -> AnyElement {
        let theme = self.theme;
        let act = self.act.clone();
        let target = path.to_path_buf();
        let save = self.act.clone();
        let saved = path.to_path_buf();

        content::box_of_code(theme)
            // Wider than a picture, because a line of code is longer than it is
            // tall and wrapping one is what makes a preview unreadable.
            .max_w(px(self.max_image.0.max(520.0)))
            .child(
                content::bar_of_code(theme)
                    .child(
                        div()
                            .id(SharedString::from(format!("open-{}", attached.id.as_str())))
                            .flex()
                            .flex_1()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .cursor_pointer()
                            .hover(|this| this.text_color(theme.text))
                            .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                                act(Act::Open(target.clone()), window, cx)
                            })
                            .child(kit::icon(IconName::File, 13.0, theme.text_muted))
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(self.spacing.small))
                                    .text_color(theme.text)
                                    .child(SharedString::from(label(attached))),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(self.spacing.small))
                                    .text_color(theme.text_muted)
                                    .child(SharedString::from(size(attached.size))),
                            ),
                    )
                    .child(kit::icon_button(
                        SharedString::from(format!("save-{}", attached.id.as_str())),
                        IconName::ArrowDown,
                        theme,
                        move |_, window, cx| save(Act::Save(saved.clone()), window, cx),
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .child(content::lines(
                        &head.code,
                        language,
                        theme,
                        self.highlights,
                        self.spacing.body,
                    ))
                    .when(head.remaining > 0, |this| {
                        this.child(
                            div()
                                .text_size(px(self.spacing.small))
                                .text_color(theme.text_muted)
                                .child(SharedString::from(match head.remaining {
                                    1 => "1 more line".to_owned(),
                                    more => format!("{more} more lines"),
                                })),
                        )
                    }),
            )
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

    /// A real fraction where there is one, and a sliding bar for the moment
    /// before the first bytes land — which is the only part of a download
    /// nothing can be said about, and is over in a few hundred milliseconds.
    fn downloading(&self, attached: &Attachment, fraction: Option<f32>) -> AnyElement {
        let theme = self.theme;
        let percent = fraction.map(|fraction| (fraction * 100.0).clamp(0.0, 100.0));

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
                        .loading(percent.is_none())
                        .value(percent.unwrap_or(0.0))
                        .color(theme.accent)
                        .with_size(gpui_component::Size::XSmall),
                    )
                    .child(
                        div()
                            .text_size(px(theme.typography.ui_size - 3.0))
                            .text_color(theme.text_muted)
                            .child(SharedString::from(match percent {
                                Some(percent) => format!(
                                    "{} of {}",
                                    size((attached.size as f32 * percent / 100.0) as u64),
                                    size(attached.size)
                                ),
                                None => format!("Downloading… {}", size(attached.size)),
                            })),
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

/// How tall the two strips are. A waveform is a picture and wants room; a rail
/// is a line and only needs enough around it to be clicked.
const WAVE_HEIGHT: f32 = 26.0;
const RAIL_HEIGHT: f32 = 16.0;

/// The shape of the sound, in the bars there is room for.
///
/// Painted rather than built out of elements. Forty-four divs each `flex_1` with
/// a margin of a pixel is forty-four boxes whose widths are whatever is left over
/// after the layout has rounded each of them to the device grid — at the widths a
/// message actually gets, a couple of pixels apiece, that is a strip of bars in
/// two different widths and gaps that come and go. Here the geometry is arrived
/// at from the bounds the strip was *given*: as many bars as fit at a fixed size,
/// spaced across the whole of it.
fn wave(
    id: String,
    waveform: Option<Vec<u8>>,
    progress: f32,
    theme: &Theme,
    act: Dispatch,
    path: std::path::PathBuf,
) -> gpui::Stateful<Div> {
    /// Signal's own proportions: a bar about as wide as the gap beside it.
    const BAR: f32 = 3.0;
    const GAP: f32 = 2.0;

    let (played, rest) = (theme.accent, unplayed(theme));

    seekable(id, WAVE_HEIGHT, act, path, move |at, window| {
        let (width, height) = (f32::from(at.size.width), f32::from(at.size.height));
        // Bounded below so a narrow window draws a waveform rather than three
        // bars, and above so a wide one does not draw hair.
        let count = (((width + GAP) / (BAR + GAP)).floor() as usize).clamp(8, 96);
        let step = (width + GAP) / count as f32;

        for (index, level) in audio::bars(waveform.as_deref(), count).into_iter().enumerate() {
            let tall = (height * level).max(2.0);
            let bounds = gpui::Bounds {
                origin: gpui::point(
                    at.origin.x + px(index as f32 * step),
                    at.origin.y + px((height - tall) / 2.0),
                ),
                size: gpui::size(px(BAR), px(tall)),
            };
            // Measured at the middle of the bar: a bar is only played once the
            // playhead has passed it, not once it has touched its leading edge.
            let done = (index as f32 + 0.5) / count as f32 <= progress;

            window.paint_quad(
                gpui::fill(bounds, if done { played } else { rest })
                    .corner_radii(px(BAR / 2.0)),
            );
        }
    })
}

/// How large the cover is drawn. Square, and tall enough for the three lines
/// beside it.
const COVER: f32 = 64.0;

/// A plain bar with the playhead on it, seekable the same way the waveform is.
/// What a track has instead of a shape, because nothing ships one for it and
/// reading one off a whole album side is not what an attachment row is for.
fn rail(
    id: String,
    progress: f32,
    theme: &Theme,
    act: Dispatch,
    path: std::path::PathBuf,
) -> gpui::Stateful<Div> {
    const THICK: f32 = 4.0;
    const KNOB: f32 = 10.0;

    let (played, rest) = (theme.accent, unplayed(theme));

    seekable(id, RAIL_HEIGHT, act, path, move |at, window| {
        let (width, height) = (f32::from(at.size.width), f32::from(at.size.height));
        let progress = progress.clamp(0.0, 1.0);
        let line = |from: f32, to: f32, colour| {
            let bounds = gpui::Bounds {
                origin: gpui::point(
                    at.origin.x + px(from),
                    at.origin.y + px((height - THICK) / 2.0),
                ),
                size: gpui::size(px(to - from), px(THICK)),
            };
            gpui::fill(bounds, colour).corner_radii(px(THICK / 2.0))
        };

        window.paint_quad(line(0.0, width, rest));
        if progress > 0.0 {
            window.paint_quad(line(0.0, width * progress, played));
        }
        // The knob is what says the bar can be moved rather than only watched.
        // Kept inside the ends, so at nought and at one it is a circle on the
        // rail rather than half a circle beside it.
        let centre = (width * progress).clamp(KNOB / 2.0, (width - KNOB / 2.0).max(KNOB / 2.0));
        window.paint_quad(
            gpui::fill(
                gpui::Bounds {
                    origin: gpui::point(
                        at.origin.x + px(centre - KNOB / 2.0),
                        at.origin.y + px((height - KNOB) / 2.0),
                    ),
                    size: gpui::size(px(KNOB), px(KNOB)),
                },
                played,
            )
            .corner_radii(px(KNOB / 2.0)),
        );
    })
}

/// The one control the speed gets: a pill saying what it is, which on a click
/// becomes the next one up. A menu of three would be a menu for a choice with
/// three values, and a row of three buttons would be three controls for one
/// setting — Signal, Podcasts and every voice-note player cycle one label.
///
/// Always drawn rather than only while something is playing: a control that
/// appears when you press play is one you find by accident, and the speed is
/// the player's, so it is as true of a note not yet started as of one running.
/// It is the speed *the player* is set to, which is why one note showing `1.5×`
/// means they all do.
fn speed_pill(id: String, speed: f32, size: f32, theme: &Theme, act: Dispatch) -> impl IntoElement {
    let faster = act.clone();

    div()
        .id(SharedString::from(id))
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        // Fixed, so cycling `1×` to `1.5×` does not move the control out from
        // under the pointer that is cycling it.
        .w(px(34.0))
        .py_0p5()
        .rounded_full()
        .cursor_pointer()
        .bg(theme.sunken)
        .text_size(px(size))
        .text_color(if speed > 1.0 {
            theme.accent
        } else {
            theme.text_muted
        })
        .hover(|this| this.bg(theme.hover))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            faster(Act::Faster, window, cx)
        })
        .child(SharedString::from(audio::speed_label(speed)))
}

/// What the part that has not been played yet is drawn in.
///
/// The accent, worn thin — the same thing, not yet — rather than a colour of its
/// own. This was `border_focus`, which in `signal-dark` *is* the accent: the
/// played half and the unplayed half were the same blue, so a voice note was a
/// solid block and a track's bar looked full from the moment it appeared. Any
/// theme is free to point two tokens at one colour; a difference of alpha is the
/// one distinction none of them can collapse.
fn unplayed(theme: &Theme) -> gpui::Hsla {
    gpui::Hsla {
        a: 0.3,
        ..theme.accent
    }
}

/// A strip that paints itself and turns a click on it into a fraction.
///
/// Both halves need the bounds and a layout closure does not know what it ended
/// up as, so a `canvas` records them: the paint closure is handed them outright,
/// and the click handler reads the ones the last paint recorded.
fn seekable(
    id: String,
    height: f32,
    act: Dispatch,
    path: std::path::PathBuf,
    draw: impl Fn(gpui::Bounds<gpui::Pixels>, &mut gpui::Window) + 'static,
) -> gpui::Stateful<Div> {
    use std::cell::Cell;
    use std::rc::Rc;

    let bounds: Rc<Cell<gpui::Bounds<gpui::Pixels>>> = Rc::new(Cell::new(gpui::Bounds::default()));
    let measured = bounds.clone();

    div()
        .id(SharedString::from(id))
        .flex()
        .flex_1()
        .min_w_0()
        .h(px(height))
        .cursor_pointer()
        .child(
            gpui::canvas(
                move |at, _, _| measured.set(at),
                move |at, _: (), window, _| draw(at, window),
            )
            .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            move |event: &gpui::MouseDownEvent, window, cx| {
                let at = bounds.get();
                if at.size.width <= px(0.0) {
                    return;
                }
                let fraction = ((event.position.x - at.origin.x) / at.size.width).clamp(0.0, 1.0);
                act(Act::Seek(path.clone(), fraction), window, cx)
            },
        )
}

/// What an audio file says it is, read once per file.
///
/// A visible row is rebuilt every frame, and reading the tags is a file read and
/// a parse -- so doing it where it is drawn would be one per frame per
/// attachment. Keyed on the metadata as well as the path, the same way the text
/// preview is, because a file being sent is one somebody may still be writing.
fn song(path: &Path) -> Option<std::rc::Rc<petunia_media::song::Song>> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    /// Enough for what is on screen and the overdraw around it.
    const CAPACITY: usize = 64;

    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

    thread_local! {
        static CACHE: RefCell<HashMap<Key, Option<Rc<petunia_media::song::Song>>>> =
            RefCell::new(HashMap::new());
    }

    let metadata = std::fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), metadata.len(), metadata.modified().ok());

    CACHE.with(|cache| {
        if let Some(known) = cache.borrow().get(&key) {
            return known.clone();
        }

        let read = petunia_media::song::read(path).map(Rc::new);
        let mut cache = cache.borrow_mut();
        if cache.len() >= CAPACITY {
            cache.clear();
        }
        cache.insert(key, read.clone());
        read
    })
}

/// What the strip draws: whatever the sender sent, and otherwise whatever can be
/// read out of the file.
///
/// The protocol's own wins outright. It is what every other client is drawing for
/// this message, it cost nothing to obtain, and a second opinion computed here
/// would be the same sound in a different shape depending on which client you
/// happened to open it in.
fn sound(waveform: Option<&[u8]>, path: &Path) -> Option<Vec<u8>> {
    match waveform.filter(|waveform| !waveform.is_empty()) {
        Some(waveform) => Some(waveform.to_vec()),
        None => shape(path).map(|shape| shape.as_ref().clone()),
    }
}

/// The shape of a sound the sender sent none for, read once per file.
///
/// Cached the way the tags are and for the same reason: a visible row is
/// rebuilt every frame, and this is a decode. Keyed on the metadata as well as
/// the path, since a file being sent is one somebody may still be writing.
fn shape(path: &Path) -> Option<std::rc::Rc<Vec<u8>>> {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::rc::Rc;

    const CAPACITY: usize = 64;

    type Key = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

    thread_local! {
        static CACHE: RefCell<HashMap<Key, Option<Rc<Vec<u8>>>>> = RefCell::new(HashMap::new());
    }

    let metadata = std::fs::metadata(path).ok()?;
    let key = (path.to_path_buf(), metadata.len(), metadata.modified().ok());

    CACHE.with(|cache| {
        if let Some(known) = cache.borrow().get(&key) {
            return known.clone();
        }

        let read = petunia_media::waveform::read(path)
            .filter(|shape| !shape.is_empty())
            .map(Rc::new);
        let mut cache = cache.borrow_mut();
        if cache.len() >= CAPACITY {
            cache.clear();
        }
        cache.insert(key, read.clone());
        read
    })
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
