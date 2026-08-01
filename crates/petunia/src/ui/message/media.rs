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
            (_, Blob::Cached(path)) => match text::language(path).zip(text::head(path)) {
                Some((language, head)) => self.text(attached, path, language, &head),
                None => self.file(attached, path),
            },
            (_, Blob::Downloading) => self.downloading(attached),
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

    /// A record as a record: the cover, what it is called, who made it, and the
    /// numbers that say what was kept of it.
    ///
    /// The waveform is left off deliberately. Signal ships one with a voice note
    /// and nothing else, so on a track it is forty-four identical grey bars — a
    /// picture of no information, standing where the cover belongs. What replaces
    /// it is a bar that fills, which is the one thing about a playing track the
    /// interface actually knows.
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
        let elapsed = match mine {
            true => Some(self.playback.position),
            false => song.duration.or_else(|| length(attached)),
        };

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

        chip_shell(theme)
            .w(px(320.0))
            .items_start()
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
                    // Rounded on the picture as well as on the well behind it:
                    // `overflow_hidden` clips a child to the parent's rectangle
                    // rather than to its corners, so a square cover in a rounded
                    // box keeps its own square corners.
                    .when(song.cover, |this| {
                        this.child(image::artwork(path, COVER).rounded(px(kit::RADIUS)))
                    })
                    // A record with no artwork gets the mark a record gets, not
                    // the bell an attached sound gets.
                    .when(!song.cover, |this| {
                        this.child(kit::glyph("icons/music.svg", 18.0, theme.text_dim))
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
                            .text_size(px(theme.typography.ui_size))
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
                                .text_size(px(self.spacing.small))
                                .text_color(theme.text_muted)
                                .child(SharedString::from(credit)),
                        )
                    })
                    .when_some(song.quality(), |this, quality| {
                        this.child(
                            div()
                                .truncate()
                                .text_size(px(self.spacing.small))
                                .text_color(theme.text_dim)
                                .child(SharedString::from(quality)),
                        )
                    })
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .pt_1()
                            .child(
                                div()
                                    .id(SharedString::from(format!(
                                        "track-{}",
                                        attached.id.as_str()
                                    )))
                                    .size(px(26.0))
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
                                        12.0,
                                        theme.on_accent,
                                    )),
                            )
                            .child(track(attached, progress, theme, seek, seek_target))
                            .when_some(elapsed, |this, elapsed| {
                                this.child(
                                    div()
                                        .flex_none()
                                        .text_size(px(self.spacing.small))
                                        .text_color(theme.text_muted)
                                        .child(SharedString::from(audio::clock(elapsed))),
                                )
                            }),
                    ),
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

/// How large the cover is drawn. Square, and tall enough for the three lines
/// beside it.
const COVER: f32 = 64.0;

/// A plain progress bar, seekable the same way the waveform is. What a track has
/// instead of a shape, because nothing ships one for it.
fn track(
    attached: &Attachment,
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
        .id(SharedString::from(format!("scrub-{}", attached.id.as_str())))
        .relative()
        .flex()
        .flex_1()
        .min_w_0()
        .items_center()
        .h(px(16.0))
        .cursor_pointer()
        .child(
            gpui::canvas(move |at, _, _| measured.set(at), |_, _: (), _, _| {})
                .absolute()
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
        .child(
            div()
                .w_full()
                .h(px(3.0))
                .rounded_full()
                .bg(theme.border_focus)
                .child(
                    div()
                        .w(gpui::relative(progress.clamp(0.0, 1.0)))
                        .h_full()
                        .rounded_full()
                        .bg(theme.accent),
                ),
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
