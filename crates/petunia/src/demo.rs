//! Fake data for a screenshot, no linked account required.
//!
//! Set `PETUNIA_DEMO=1` to skip the real Signal worker and populate the store
//! with a handful of made-up threads instead. Never wired to anything a real
//! run touches.

use std::path::PathBuf;

use gpui::{App, Entity};
use uuid::Uuid;

use petunia_data::{self as data, Connection, Contact, ContactId, MessageId, Thread};
use petunia_signal::Event;

use crate::store::Store;

pub fn enabled() -> bool {
    std::env::var("PETUNIA_DEMO").is_ok_and(|value| value == "1")
}

pub fn install(store: Entity<Store>, cx: &mut App) {
    let me = uuid("11111111-1111-1111-1111-111111111111");
    let fern = uuid("22222222-2222-2222-2222-222222222222");
    let stark = uuid("33333333-3333-3333-3333-333333333333");

    let avatars_dir = PathBuf::from("/tmp/frieren_pfps");

    store.update(cx, |store, cx| {
        store.apply(
            Event::Linked {
                aci: me,
                phone_number: "+15555550100".into(),
            },
            cx,
        );
        store.apply(
            Event::Profile {
                uuid: me,
                name: "Frieren".into(),
            },
            cx,
        );
        if let Some(state) = store.state_mut() {
            state.show_own_name = true;
            state.connection = Connection::Connected;
        }

        store.apply(
            Event::Contacts {
                contacts: vec![contact(fern, "Fern"), contact(stark, "Stark")],
                groups: vec![],
            },
            cx,
        );

        for (thread, name) in [
            (Thread::Contact(ContactId::Aci(me)), "frieren"),
            (Thread::Contact(ContactId::Aci(fern)), "fern"),
            (Thread::Contact(ContactId::Aci(stark)), "stark"),
        ] {
            store.apply(
                Event::Avatar {
                    thread,
                    path: avatars_dir.join(format!("{name}_face.png")),
                },
                cx,
            );
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        page(
            store,
            cx,
            Thread::Contact(ContactId::Aci(stark)),
            now - 3 * 60 * 60 * 1_000,
            &[
                (stark, "There's a monster nest past the ridge, I can handle it"),
                (me, "Go ahead. Scream if it's serious."),
                (stark, "I wasn't screaming last time"),
                (me, "You were audible from the river."),
            ],
        );

        page(
            store,
            cx,
            Thread::Contact(ContactId::Aci(fern)),
            now - 8 * 60 * 1_000,
            &[
                (fern, "We're almost at the next town. Do you want to stop for the night?"),
                (me, "We have another two hours of daylight. Let's keep moving."),
                (fern, "You said that three hours ago."),
                (me, "Did I."),
                (fern, "I'm putting the tent up."),
            ],
        );

        store.activate(Thread::Contact(ContactId::Aci(fern)), cx);
    });
}

fn uuid(s: &str) -> Uuid {
    s.parse().unwrap()
}

fn contact(uuid: Uuid, name: &str) -> Contact {
    Contact {
        uuid,
        name: name.into(),
    }
}

fn page(
    store: &mut Store,
    cx: &mut gpui::Context<Store>,
    thread: Thread,
    start: u64,
    messages: &[(Uuid, &str)],
) {
    let built: Vec<data::Message> = messages
        .iter()
        .enumerate()
        .map(|(i, (sender, body))| {
            data::Message::plain(
                MessageId {
                    timestamp: start + i as u64 * 60_000,
                    sender: *sender,
                },
                (*body).into(),
            )
        })
        .collect();

    store.apply(
        Event::History {
            thread,
            messages: built,
            more: false,
            covered: None,
            older: false,
        },
        cx,
    );
}
