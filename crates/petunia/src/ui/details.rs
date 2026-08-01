use std::path::PathBuf;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{Context, Entity, MouseButton, SharedString, Window, div, px};

use uuid::Uuid;

use super::avatar::avatar;
use super::image;
use super::kit;
use petunia_config::Theme;
use petunia_data::attachment::{Blob, Kind};
use petunia_data::{Member, State, Thread};
use crate::store::{Focus, Store};
use crate::theme::ActivePalette;

/// Something in the panel was asked to open full size. The workspace owns the
/// viewer, so the panel only says what was clicked.
#[derive(Debug, Clone)]
pub struct Viewing(pub PathBuf);

/// A nickname was asked for. The workspace owns the prompt that takes a line
/// of text, same as naming a folder.
#[derive(Debug, Clone, Copy)]
pub struct EditNickname(pub Uuid);

/// Somebody is to be blocked or unblocked. The workspace answers it, because
/// blocking asks first and a panel does not own dialogs.
pub struct SetBlocked(pub Uuid, pub bool);

impl gpui::EventEmitter<Viewing> for Details {}
impl gpui::EventEmitter<EditNickname> for Details {}
impl gpui::EventEmitter<SetBlocked> for Details {}

/// The two things anything in the panel can ask for. Built before the store is
/// read, because a listener needs the context mutably and the store's contents
/// are borrowed out of it.
#[derive(Clone)]
struct Hooks {
    view: Hook<PathBuf>,
    inspect: Hook<Uuid>,
    nickname: Hook<Uuid>,
    /// Who, and which way. Blocking is asked about and unblocking is not, and
    /// that decision belongs to the workspace rather than to a row.
    block: Hook<(Uuid, bool)>,
}

type Hook<T> = std::rc::Rc<dyn Fn(T, &mut Window, &mut gpui::App)>;

/// Who or what the conversation is, and what has been shared in it.
pub struct Details {
    store: Entity<Store>,
}

impl Details {
    pub fn new(store: Entity<Store>, cx: &mut Context<Self>) -> Self {
        cx.observe(&store, |_, _, cx| cx.notify()).detach();
        Self { store }
    }

    fn hooks(&self, cx: &mut Context<Self>) -> Hooks {
        let this = cx.entity();
        let store = self.store.clone();

        Hooks {
            view: std::rc::Rc::new(move |path, _, cx| {
                this.update(cx, |_, cx| cx.emit(Viewing(path)));
            }),
            inspect: std::rc::Rc::new(move |uuid, _, cx| {
                store.update(cx, |store, cx| store.inspect(Some(Focus::Person(uuid)), cx));
            }),
            nickname: {
                let this = cx.entity();
                std::rc::Rc::new(move |uuid, _, cx| {
                    this.update(cx, |_, cx| cx.emit(EditNickname(uuid)));
                })
            },
            block: {
                let this = cx.entity();
                std::rc::Rc::new(move |(uuid, blocked), _, cx| {
                    this.update(cx, |_, cx| cx.emit(SetBlocked(uuid, blocked)));
                })
            },
        }
    }
}

impl Render for Details {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let palette = cx.palette().clone();
        let hooks = self.hooks(cx);
        let store = self.store.read(cx);

        let Some(state) = store.state() else {
            return shell(&palette, div());
        };

        let body = match store.focus() {
            Some(Focus::Person(uuid)) => person(*uuid, store.active(), state, &palette, &hooks),
            None => match store.active() {
                Some(thread) => conversation(thread, state, &palette, &hooks),
                None => div()
                    .p_4()
                    .text_size(px(palette.typography.ui_size))
                    .text_color(palette.text_muted)
                    .child("Nothing selected."),
            },
        };

        shell(&palette, body)
    }
}

fn shell(palette: &Theme, body: gpui::Div) -> gpui::Stateful<gpui::Div> {
    div()
        .id("details")
        .size_full()
        .overflow_y_scroll()
        .bg(palette.surface)
        .text_color(palette.text)
        .child(body)
}

fn person(
    uuid: Uuid,
    thread: Option<&Thread>,
    state: &State,
    palette: &Theme,
    hooks: &Hooks,
) -> gpui::Div {
    let name = state.name_of(uuid);
    let picture = state.avatar_for(uuid).map(|path| path.to_path_buf());
    // What the group calls them, which is not their profile name: Signal lets
    // members pick a short label and an emoji for themselves.
    let badge = thread
        .and_then(|thread| state.member(thread, uuid))
        .and_then(Member::badge);

    div()
        .flex()
        .flex_col()
        .child(hero(picture, &name, uuid.as_bytes(), palette, hooks))
        .when_some(badge, |this, badge| {
            this.child(
                div()
                    .flex()
                    .justify_center()
                    .pb_3()
                    .child(kit::badge(
                        badge,
                        palette.accent_for(uuid.as_bytes()),
                        palette.typography.ui_size - 1.0,
                        palette.typography.ui_size * 1.6,
                    )),
            )
        })
        .child(fields(
            [
                Some(("Name", name.clone())),
                Some(("Account", uuid.to_string())),
                (uuid == state.aci).then(|| ("This is", "you".to_string())),
            ],
            palette,
        ))
        .when(uuid != state.aci, |this| {
            this.child(nickname_row(uuid, state.nickname_of(uuid), palette, hooks))
                .child(block_row(uuid, state.is_blocked(uuid), palette, hooks))
        })
}

/// Blocking, as a row rather than a switch: it is one of two verbs depending on
/// where you are, and a toggle that has to be read to know which way it is
/// pointing is a control that says less than the word does.
fn block_row(
    uuid: Uuid,
    blocked: bool,
    palette: &Theme,
    hooks: &Hooks,
) -> impl IntoElement {
    let go = hooks.block.clone();
    let tint = match blocked {
        true => palette.text_dim,
        false => palette.danger,
    };

    div()
        .id("block")
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .py_2()
        .cursor_pointer()
        .hover(|this| this.bg(palette.hover))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            go((uuid, !blocked), window, cx)
        })
        .child(
            div()
                .text_size(px(palette.typography.ui_size))
                .text_color(tint)
                .child(match blocked {
                    true => "Unblock",
                    false => "Block…",
                }),
        )
        .children(blocked.then(|| {
            div()
                .text_size(px(palette.typography.ui_size - 1.0))
                .text_color(palette.text_muted)
                .child("Blocked")
        }))
}

/// A nickname is set through this account alone -- there is nothing else here
/// worth a text field of its own, so it borrows the same one-line prompt a new
/// folder is named with.
fn nickname_row(
    uuid: Uuid,
    nickname: Option<&str>,
    palette: &Theme,
    hooks: &Hooks,
) -> impl IntoElement {
    let go = hooks.nickname.clone();

    div()
        .id("nickname")
        .flex()
        .items_center()
        .justify_between()
        .px_4()
        .py_2()
        .cursor_pointer()
        .hover(|this| this.bg(palette.hover))
        .on_mouse_down(MouseButton::Left, move |_, window, cx| go(uuid, window, cx))
        .child(
            div()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text_dim)
                .child("Nickname"),
        )
        .child(
            div()
                .text_size(px(palette.typography.ui_size))
                .text_color(palette.text)
                .child(nickname.map(str::to_owned).unwrap_or_else(|| "Set…".into())),
        )
}

fn conversation(
    thread: &Thread,
    state: &State,
    palette: &Theme,
    hooks: &Hooks,
) -> gpui::Div {
    let name = state.title(thread);
    let group = state.group(thread);
    let shared: Vec<_> = state
        .history(thread)
        .map(|history| {
            history
                .messages()
                .iter()
                .rev()
                .flat_map(|message| message.attachments.iter())
                .filter(|attached| matches!(attached.kind, Kind::Image { .. }))
                .filter_map(|attached| match &attached.blob {
                    Blob::Cached(path) => Some(path.clone()),
                    _ => None,
                })
                .take(12)
                .collect()
        })
        .unwrap_or_default();

    div()
        .flex()
        .flex_col()
        .child(hero(
            state.avatar(thread).map(|path| path.to_path_buf()),
            &name,
            thread.seed(),
            palette,
            hooks,
        ))
        .when_some(group.and_then(|group| group.description.clone()), |this, about| {
            this.child(
                div()
                    .px_4()
                    .pb_3()
                    .text_size(px(palette.typography.ui_size - 1.0))
                    .text_color(palette.text_dim)
                    .child(SharedString::from(about)),
            )
        })
        .child(fields(
            [
                Some((
                    "Kind",
                    match thread {
                        Thread::Contact(_) => "Direct message".to_string(),
                        Thread::Group(_) => "Group".to_string(),
                    },
                )),
                group.map(|group| ("Members", group.members.len().to_string())),
                group
                    .filter(|group| group.invited > 0)
                    .map(|group| ("Invited", group.invited.to_string())),
                group
                    .filter(|group| group.requesting > 0)
                    .map(|group| ("Requests", group.requesting.to_string())),
                group
                    .and_then(|group| group.expire_timer)
                    .map(|timer| ("Disappearing", timer_label(timer))),
            ],
            palette,
        ))
        .when_some(group, |this, group| {
            this.child(members(group, state, palette, hooks))
        })
        .when(!shared.is_empty(), |this| {
            this.child(
                div()
                    .flex()
                    .flex_col()
                    .px_4()
                    .pb_4()
                    .child(kit::section("Shared media", palette))
                    .child(
                        div()
                            .flex()
                            .flex_wrap()
                            .gap_1p5()
                            .children(shared.into_iter().enumerate().map(|(index, path)| {
                                let target = path.clone();
                                let view = hooks.view.clone();
                                div()
                                    .id(SharedString::from(format!("shared-{index}")))
                                    .flex_none()
                                    .cursor_pointer()
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        move |_, window, cx| view(target.clone(), window, cx),
                                    )
                                    .child(image::cropped(&path, 62.0).rounded(px(kit::RADIUS)))
                            })),
                    ),
            )
        })
}

/// Everyone in the group, administrators first, each with whatever the group
/// says about them.
fn members(
    group: &petunia_data::Group,
    state: &State,
    palette: &Theme,
    hooks: &Hooks,
) -> gpui::Div {
    let listed = group.ordered(|uuid| state.name_of(uuid));

    div()
        .flex()
        .flex_col()
        .px_4()
        .pb_2()
        .child(kit::section(
            SharedString::from(format!("{} members", listed.len())),
            palette,
        ))
        .children(listed.into_iter().map(|(member, name)| {
            let uuid = member.uuid;
            let role = member.role.label();
            let badge = member.badge();
            let inspect = hooks.inspect.clone();

            div()
                .id(SharedString::from(format!("member-{uuid}")))
                .flex()
                .items_center()
                .gap_2p5()
                .px_1()
                .py_1p5()
                .rounded(px(kit::RADIUS))
                .cursor_pointer()
                .hover(|this| this.bg(palette.hover))
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    inspect(uuid, window, cx)
                })
                .child(avatar(
                    state.avatar_for(uuid),
                    &name,
                    uuid.as_bytes(),
                    28.0,
                    palette,
                ))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .child(
                            div()
                                .truncate()
                                .text_size(px(palette.typography.ui_size))
                                .text_color(palette.text)
                                .child(SharedString::from(if uuid == state.aci {
                                    format!("{name} (you)")
                                } else {
                                    name.clone()
                                })),
                        )
                        .when_some(badge, |this, badge| {
                            this.child(
                                div()
                                    .truncate()
                                    .text_size(px(palette.typography.ui_size - 3.0))
                                    .text_color(palette.accent_for(uuid.as_bytes()))
                                    .child(SharedString::from(badge)),
                            )
                        }),
                )
                // The same badge the message list draws, so the group's word for
                // somebody looks like one thing wherever it appears.
                .when_some(role, |this, role| {
                    this.child(kit::badge(
                        role,
                        palette.text_dim,
                        palette.typography.ui_size - 2.0,
                        palette.typography.ui_size * 1.5,
                    ))
                })
        }))
}

/// The picture at the top, which opens full size like any other.
fn hero(
    picture: Option<PathBuf>,
    name: &str,
    seed: &[u8],
    palette: &Theme,
    hooks: &Hooks,
) -> gpui::Div {
    let target = picture.clone();
    let view = hooks.view.clone();

    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2p5()
        .px_4()
        .pt_6()
        .pb_5()
        .child(
            div()
                .id("hero")
                .flex_none()
                .when(target.is_some(), |this| {
                    this.cursor_pointer()
                        .tooltip(|window, cx| {
                            gpui_component::tooltip::Tooltip::new("View full size")
                                .build(window, cx)
                        })
                        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                            if let Some(target) = target.clone() {
                                view(target, window, cx);
                            }
                        })
                })
                .child(avatar(picture.as_deref(), name, seed, 72.0, palette)),
        )
        .child(
            div()
                .text_size(px(palette.typography.ui_size + 3.0))
                .text_color(palette.text)
                .child(SharedString::from(name.to_owned())),
        )
}

/// Rows with nothing to say are left out rather than shown empty.
fn fields<const N: usize>(
    rows: [Option<(&'static str, String)>; N],
    palette: &Theme,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .px_4()
        .pb_2()
        .children(rows.into_iter().flatten().map(|(label, value)| {
            div()
                .flex()
                .items_baseline()
                .justify_between()
                .gap_3()
                .py_1p5()
                .border_b_1()
                .border_color(palette.border)
                .child(
                    div()
                        .flex_none()
                        .text_size(px(palette.typography.ui_size - 2.0))
                        .text_color(palette.text_muted)
                        .child(label),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(px(palette.typography.ui_size - 1.0))
                        .text_color(palette.text_dim)
                        .child(SharedString::from(value)),
                )
        }))
}

/// A disappearing-message timer as the phone says it, not as a number of
/// seconds.
fn timer_label(timer: Duration) -> String {
    let seconds = timer.as_secs();
    const WEEK: u64 = 7 * 24 * 3600;

    match seconds {
        0 => "Off".into(),
        seconds if seconds % WEEK == 0 => plural(seconds / WEEK, "week"),
        seconds if seconds % 86_400 == 0 => plural(seconds / 86_400, "day"),
        seconds if seconds % 3600 == 0 => plural(seconds / 3600, "hour"),
        seconds if seconds % 60 == 0 => plural(seconds / 60, "minute"),
        seconds => plural(seconds, "second"),
    }
}

fn plural(count: u64, unit: &str) -> String {
    match count {
        1 => format!("1 {unit}"),
        count => format!("{count} {unit}s"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timer_reads_in_the_largest_whole_unit() {
        assert_eq!(timer_label(Duration::from_secs(0)), "Off");
        assert_eq!(timer_label(Duration::from_secs(30)), "30 seconds");
        assert_eq!(timer_label(Duration::from_secs(300)), "5 minutes");
        assert_eq!(timer_label(Duration::from_secs(3600)), "1 hour");
        assert_eq!(timer_label(Duration::from_secs(86_400)), "1 day");
        assert_eq!(timer_label(Duration::from_secs(7 * 86_400)), "1 week");
        assert_eq!(timer_label(Duration::from_secs(28 * 86_400)), "4 weeks");
    }
}
