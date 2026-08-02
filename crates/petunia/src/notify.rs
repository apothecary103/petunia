//! Telling the desktop something arrived.
//!
//! What `[notifications]` in `config.toml` has always described and nothing has
//! ever read. The policy lives in `wanted` — a pure function over the preference,
//! the thread and the message — so it can be tested without a notification centre
//! and without a window; posting is the two lines at the bottom.
//!
//! Nothing is posted for the conversation you are looking at in a window you are
//! looking at. A banner about the message already on screen is a banner about
//! nothing, and it is the one case a notification is guaranteed to be wrong.

use std::sync::OnceLock;
use std::sync::mpsc::{self, Sender};

use petunia_config::{GroupNotifications, Notifications};
use petunia_data::{Message, State, Thread};

/// A notification to post: what it says, and which conversation it came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    pub title: String,
    pub body: String,
    /// Where it came from, so clicking the banner can open it. A banner about a
    /// message that leaves you to find the message is half a notification.
    pub thread: Thread,
    /// The face beside it, which is the sender's in a one-to-one and the
    /// group's in a group — the same picture the sidebar row carries.
    pub picture: Option<std::path::PathBuf>,
}

/// Whether this message earns a notification, and what it would say.
///
/// `attending` is "this conversation is on screen in a window with the focus".
/// Everything else is the preference and what the message is.
pub fn wanted(
    settings: &Notifications,
    state: &State,
    thread: &Thread,
    message: &Message,
    attending: bool,
    now: u64,
) -> Option<Notice> {
    if !settings.enabled || attending || !worth_telling(settings, state, thread, message, now) {
        return None;
    }

    let who = state.name_of(message.sender());
    let where_ = state.title(thread);
    // In a group, both: which conversation is as much of the answer as who, and
    // one without the other has you opening the app to find out.
    let title = match (settings.show_sender, thread.is_group()) {
        (true, true) => format!("{who} in {where_}"),
        (true, false) => who,
        (false, true) => where_,
        (false, false) => "Petunia".to_owned(),
    };
    let body = match settings.show_content {
        true => message.summary(),
        // Deliberately not "1 new message": a count implies this is a digest of
        // several, and it is one banner per message.
        false => "New message".to_owned(),
    };

    let picture = match thread.is_group() {
        true => state.avatar(thread),
        false => state.avatar_for(message.sender()),
    };

    Some(Notice {
        title,
        body,
        thread: thread.clone(),
        picture: picture.map(std::path::Path::to_path_buf),
    })
}

/// Whether this message is anybody's business but the store's — mine, a
/// tombstone, a muted conversation, a group this account has told to be quiet.
/// Shared by the banner and the sound, because a sound *is* a notification with
/// no words in it, and two policies that can disagree is a conversation that is
/// muted for one of them.
fn worth_telling(
    settings: &Notifications,
    state: &State,
    thread: &Thread,
    message: &Message,
    now: u64,
) -> bool {
    // Our own message, arriving as the echo of a send or as a sync from another
    // device. Notifying somebody about what they just typed is absurd.
    if message.sender() == state.aci {
        return false;
    }
    // A tombstone, a reaction's own row, or anything else with nothing to say.
    if !message.is_addressable() {
        return false;
    }
    if state.index.flags(thread).muted(now) {
        return false;
    }
    if thread.is_group() {
        let mentioned = message.mentions(state.aci);
        return match settings.groups {
            GroupNotifications::None => false,
            GroupNotifications::Mentions => mentioned,
            GroupNotifications::All => true,
        };
    }
    true
}

/// Whether a message arriving is worth a sound.
///
/// Unlike a banner this does not care whether the conversation is on screen: a
/// tone is the answer to "did that send" and "did something come in" while you
/// are looking straight at the thread, which is exactly when a banner would be
/// wrong and a sound is right.
pub fn audible(
    settings: &Notifications,
    state: &State,
    thread: &Thread,
    message: &Message,
    now: u64,
) -> bool {
    settings.sounds && worth_telling(settings, state, thread, message, now)
}

/// Where a clicked banner goes. Set once by the workspace, which is the only
/// thing that can open a conversation, and read from the thread the banners are
/// posted on — the notification centre answers on its own schedule and the
/// answer has to cross back.
static OPENING: OnceLock<futures::channel::mpsc::UnboundedSender<Thread>> = OnceLock::new();

pub fn opens_with(sender: futures::channel::mpsc::UnboundedSender<Thread>) {
    let _ = OPENING.set(sender);
}

/// Hands the notice to the desktop.
///
/// Failures are logged and nothing else. A notification centre that has been
/// told not to show anything from this application is a preference the person
/// set, and it is not the message list's problem.
/// Posted from a thread of its own, and never from the one drawing the window.
/// macOS delivers a banner asynchronously and the backend waits for the
/// confirmation — on the main thread by spinning a *nested* run loop for up to
/// two seconds, which is the window frozen for two seconds per message with
/// gpui's `App` still borrowed underneath it, so every AppKit callback that
/// arrives in the meantime logs `RefCell already borrowed` and is dropped. Off
/// the main thread the same wait is a condvar, which blocks nobody.
///
/// A thread *each*, though, which the single queue this used to be could not
/// be: waiting to be told the banner was clicked is waiting for as long as the
/// banner is up, and one queue would then post the second message's banner
/// after the first had timed out. The dispatcher is what keeps the ordering the
/// notification centre sees; the waits happen beside each other.
pub fn post(notice: &Notice) {
    static POSTING: OnceLock<Sender<Notice>> = OnceLock::new();

    let sender = POSTING.get_or_init(|| {
        let (sender, receiver) = mpsc::channel::<Notice>();
        std::thread::spawn(move || {
            for notice in receiver {
                std::thread::Builder::new()
                    .name("petunia-notification".into())
                    .spawn(move || show(notice))
                    .ok();
            }
        });
        sender
    });

    if sender.send(notice.clone()).is_err() {
        tracing::debug!("the notifying thread is gone");
    }
}

fn show(notice: Notice) {
    let mut notification = notify_rust::Notification::new();
    notification
        .summary(&notice.title)
        .body(&notice.body)
        .appname("Petunia");

    if let Some(picture) = notice.picture.as_ref() {
        notification.image_path(&picture.to_string_lossy());
    }

    let shown = notification.show();
    let handle = match shown {
        Ok(handle) => handle,
        Err(error) => {
            tracing::debug!(%error, "could not post a notification");
            return;
        }
    };

    // Blocks until the banner is answered or gives up. "__closed" is the
    // dismissal and every other identifier is somebody having pressed it, so
    // only the click opens anything.
    handle.wait_for_action(|action| {
        if action != "default" {
            return;
        }
        let Some(opening) = OPENING.get() else {
            return;
        };
        if opening.unbounded_send(notice.thread).is_err() {
            tracing::debug!("nothing is listening for a clicked notification");
        }
    });
}

/// Names the application the notifications come from, once, before any of them
/// are posted.
///
/// Without this, macOS asks *who is asking* — with a file chooser. The backend
/// needs a bundle identifier and looks one up by name if it has not been given
/// one; the name it looks up is the string `use_default`, there is no
/// application called that, and an unresolvable application reference on macOS
/// opens the "Choose Application" panel. So every notification put a chooser on
/// screen naming a document nobody has ever had, which is the whole of "a file
/// chooser keeps appearing at random".
///
/// The identifier is this process's own when petunia is a bundle. When it is not
/// — a binary run from a shell — there is none to have, and the fallback is an
/// application that certainly exists: the notification arrives under somebody
/// else's name, which is worse than it might be and better than a dialog.
pub fn name_the_application() {
    #[cfg(target_os = "macos")]
    {
        let bundle = mac::identifier().unwrap_or_else(|| "com.apple.Finder".to_owned());
        if let Err(error) = notify_rust::set_application(&bundle) {
            tracing::debug!(%error, %bundle, "could not name the notifying application");
        }
    }
}

#[cfg(target_os = "macos")]
mod mac {
    /// This process's bundle identifier, or `None` when it is not running from a
    /// bundle.
    pub fn identifier() -> Option<String> {
        objc2_foundation::NSBundle::mainBundle()
            .bundleIdentifier()
            .map(|identifier| identifier.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use petunia_data::{ContactId, MessageId};
    use uuid::Uuid;

    fn state() -> State {
        State::new(Uuid::from_u128(1))
    }

    fn thread() -> Thread {
        Thread::Contact(ContactId::Aci(Uuid::from_u128(2)))
    }

    fn from(sender: Uuid, body: &str) -> Message {
        Message::plain(
            MessageId {
                timestamp: 1_000,
                sender,
            },
            body.into(),
        )
    }

    fn settings() -> Notifications {
        Notifications::default()
    }

    #[test]
    fn a_message_from_somebody_else_is_worth_a_banner() {
        let notice = wanted(
            &settings(),
            &state(),
            &thread(),
            &from(Uuid::from_u128(2), "hi"),
            false,
            0,
        );

        assert!(notice.is_some());
        assert_eq!(notice.unwrap().body, "hi");
    }

    /// The one case a notification is certainly wrong.
    #[test]
    fn the_conversation_on_screen_gets_none() {
        assert!(
            wanted(
                &settings(),
                &state(),
                &thread(),
                &from(Uuid::from_u128(2), "hi"),
                true,
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn our_own_message_gets_none() {
        assert!(
            wanted(
                &settings(),
                &state(),
                &thread(),
                &from(Uuid::from_u128(1), "hi"),
                false,
                0,
            )
            .is_none()
        );
    }

    #[test]
    fn disabled_means_disabled() {
        let mut settings = settings();
        settings.enabled = false;

        assert!(
            wanted(&settings, &state(), &thread(), &from(Uuid::from_u128(2), "hi"), false, 0)
                .is_none()
        );
    }

    /// Withheld content still notifies -- that something arrived is the part
    /// somebody who turned the preview off still wants.
    #[test]
    fn content_can_be_withheld_without_withholding_the_notification() {
        let mut settings = settings();
        settings.show_content = false;

        let notice = wanted(
            &settings,
            &state(),
            &thread(),
            &from(Uuid::from_u128(2), "secret"),
            false,
            0,
        )
        .expect("still notified");

        assert_eq!(notice.body, "New message");
        assert!(!notice.body.contains("secret"));
    }

    #[test]
    fn a_withheld_sender_is_not_in_the_title() {
        let mut settings = settings();
        settings.show_sender = false;

        let notice = wanted(
            &settings,
            &state(),
            &thread(),
            &from(Uuid::from_u128(2), "hi"),
            false,
            0,
        )
        .expect("still notified");

        assert_eq!(notice.title, "Petunia");
    }

    #[test]
    fn a_muted_conversation_gets_none() {
        let mut state = state();
        let thread = thread();
        // The index only carries flags for a conversation it has heard of, so
        // there has to be one before it can be muted.
        state.record(&thread, &from(Uuid::from_u128(2), "earlier"));
        state.index.set_flags(
            &thread,
            petunia_data::index::Flags {
                muted_until: Some(u64::MAX),
                ..Default::default()
            },
        );

        assert!(
            wanted(&settings(), &state, &thread, &from(Uuid::from_u128(2), "hi"), false, 0)
                .is_none()
        );
    }

    /// A tombstone is a row, not something anybody wants told about.
    #[test]
    fn an_unaddressable_message_gets_none() {
        let mut deleted = from(Uuid::from_u128(2), "");
        deleted.content = petunia_data::message::Content::Deleted;

        assert!(wanted(&settings(), &state(), &thread(), &deleted, false, 0).is_none());
    }
}
