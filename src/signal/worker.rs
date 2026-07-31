use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt, future, pin_mut};
use presage::Manager;
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{Content, DataMessage, GroupContextV2, Metadata};
use presage::libsignal_service::protocol::ServiceId;
use presage::libsignal_service::zkgroup::profiles::ProfileKey;
use presage::manager::Registered;
use presage::model::messages::Received;
use presage::store::{ContentExt, ContentsStore, StateStore};
use presage_store_sqlite::SqliteStore;
use tokio::sync::mpsc::{UnboundedReceiver, unbounded_channel};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::db::Db;
use super::{Command, Error, Event, store};
use crate::data::{self, Contact, ContactId, Group, Thread};

type Events = mpsc::Sender<Event>;
type RegisteredManager = Manager<SqliteStore, Registered>;

pub fn run(events: Events) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build signal worker runtime");
    let local = tokio::task::LocalSet::new();
    local.block_on(&runtime, start(events));
}

async fn start(mut events: Events) {
    let (commands_tx, commands_rx) = unbounded_channel();
    emit(&mut events, Event::Ready(commands_tx)).await;

    if let Err(error) = serve(commands_rx, events.clone()).await {
        error!(%error, "signal worker failed");
        emit(&mut events, Event::Error(error.to_string())).await;
    }
}

async fn emit(events: &mut Events, event: Event) {
    if let Err(error) = events.send(event).await {
        error!(%error, "failed to deliver event to the UI");
    }
}

async fn serve(mut commands: UnboundedReceiver<Command>, mut events: Events) -> Result<(), Error> {
    let store = store::open().await?;
    let db = Db::open().await?;
    match db.fail_stale_sends().await {
        Ok(0) => {}
        Ok(swept) => warn!(swept, "marked sends left in flight by a previous run as failed"),
        Err(error) => warn!(%error, "failed to sweep stale sends"),
    }
    let (mut manager, freshly_linked) = if store.is_registered().await {
        (Manager::load_registered(store).await?, false)
    } else {
        (link(store, events.clone()).await?, true)
    };

    let aci = manager.registration_data().service_ids.aci;
    info!(%aci, "signal manager ready");
    emit(&mut events, Event::Linked { aci }).await;
    if let Err(error) = send_contacts(&manager, &mut events).await {
        warn!(%error, "failed to load contacts");
    }
    tokio::task::spawn_local(fetch_avatars(manager.clone(), events.clone()));
    tokio::task::spawn_local(fetch_previews(db.clone(), events.clone()));
    if freshly_linked
        && let Err(error) = manager.request_contacts().await
    {
        warn!(%error, "failed to request contact sync");
    }

    let (queue_tx, mut queue_rx) = oneshot::channel();
    tokio::task::spawn_local(receive(
        manager.clone(),
        db.clone(),
        events.clone(),
        queue_tx,
    ));

    let mut queue_drained = false;
    let mut pending = Vec::new();
    loop {
        tokio::select! {
            _ = &mut queue_rx, if !queue_drained => {
                queue_drained = true;
                info!(pending = pending.len(), "message queue drained");
                for (thread, body, timestamp) in pending.drain(..) {
                    send(&mut manager, &db, &mut events, thread, body, timestamp).await;
                }
            }
            command = commands.recv() => match command {
                Some(Command::SendText { thread, body, timestamp }) => {
                    save_outgoing(&manager, &db, &thread, &body, timestamp).await;
                    if queue_drained {
                        send(&mut manager, &db, &mut events, thread, body, timestamp).await;
                    } else {
                        pending.push((thread, body, timestamp));
                    }
                }
                Some(Command::LoadThread { thread, before }) => {
                    match load_history(&manager, &db, &thread, aci, before).await {
                        Ok((messages, more)) => {
                            emit(&mut events, Event::History {
                                thread,
                                messages,
                                more,
                                older: before.is_some(),
                            })
                            .await;
                        }
                        Err(error) => {
                            error!(%error, "failed to load message history");
                            emit(
                                &mut events,
                                Event::Error(format!("failed to load history: {error}")),
                            )
                            .await;
                        }
                    }
                }
                None => break,
            },
        }
    }
    Ok(())
}

async fn link(store: SqliteStore, mut events: Events) -> Result<RegisteredManager, Error> {
    let (url_tx, url_rx) = oneshot::channel();
    let (manager, ()) = future::join(
        Manager::link_secondary_device(
            store,
            SignalServers::Production,
            "petunia".into(),
            url_tx,
        ),
        async move {
            if let Ok(url) = url_rx.await {
                emit(&mut events, Event::LinkUrl(url.to_string())).await;
            }
        },
    )
    .await;
    Ok(manager?)
}

async fn receive(
    mut manager: RegisteredManager,
    db: Db,
    mut events: Events,
    queue_tx: oneshot::Sender<()>,
) {
    let mut queue_signal = Some(queue_tx);
    loop {
        match receive_once(&mut manager, &db, &mut events, &mut queue_signal).await {
            Ok(()) => warn!("message stream ended, reconnecting"),
            Err(error) => {
                error!(%error, "failed to receive messages");
                emit(
                    &mut events,
                    Event::Error(format!("failed to receive messages: {error}")),
                )
                .await;
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

async fn receive_once(
    manager: &mut RegisteredManager,
    db: &Db,
    events: &mut Events,
    queue_signal: &mut Option<oneshot::Sender<()>>,
) -> Result<(), Error> {
    let messages = manager.receive_messages().await?;
    pin_mut!(messages);
    info!("message stream started");

    while let Some(received) = messages.next().await {
        match received {
            Received::QueueEmpty => {
                debug!("message queue empty");
                if let Some(signal) = queue_signal.take() {
                    let _ = signal.send(());
                }
            }
            Received::Contacts => {
                info!("contacts synced");
                if let Err(error) = send_contacts(manager, events).await {
                    warn!(%error, "failed to load synced contacts");
                }
                tokio::task::spawn_local(fetch_avatars(manager.clone(), events.clone()));
                tokio::task::spawn_local(fetch_previews(db.clone(), events.clone()));
            }
            Received::Content(content) => {
                debug!(timestamp = content.timestamp(), "received content");
                if let Some((timestamps, status)) = data::receipt_from_content(&content) {
                    let recipient = content.metadata.sender.raw_uuid();
                    if let Err(error) = db.record_receipts(&timestamps, recipient, status).await {
                        warn!(%error, "failed to save receipt statuses");
                    }
                    emit(events, Event::MessageStatus { timestamps, status }).await;
                } else if let Some((thread, message)) = data::from_content(&content) {
                    emit(events, Event::Message { thread, message }).await;
                }
            }
        }
    }
    Ok(())
}

fn text_message(thread: &Thread, body: String, timestamp: u64) -> DataMessage {
    let group_v2 = match thread {
        Thread::Contact(_) => None,
        Thread::Group(master_key) => Some(GroupContextV2 {
            master_key: Some(master_key.to_vec()),
            revision: Some(0),
            ..Default::default()
        }),
    };
    DataMessage {
        body: Some(body),
        timestamp: Some(timestamp),
        group_v2,
        ..Default::default()
    }
}

async fn save_outgoing(
    manager: &RegisteredManager,
    db: &Db,
    thread: &Thread,
    body: &str,
    timestamp: u64,
) {
    let aci = manager.registration_data().service_ids.aci;
    let time = chrono::DateTime::from_timestamp_millis(timestamp as i64).unwrap_or_default();
    let destination = match thread {
        Thread::Contact(contact) => contact.into(),
        Thread::Group(_) => ServiceId::Aci(aci.into()),
    };
    let content = Content::from_body(
        text_message(thread, body.to_string(), timestamp),
        Metadata {
            sender: ServiceId::Aci(aci.into()),
            destination,
            sender_device: manager.device_id(),
            timestamp: time,
            server_timestamp: time,
            needs_receipt: false,
            unidentified_sender: false,
            was_plaintext: false,
            server_guid: None,
        },
    );

    if let Err(error) = manager.store().save_message(&thread.into(), content).await {
        warn!(%error, "failed to save outgoing message");
    }
    if let Err(error) = db
        .set_send_state(timestamp, thread, data::Status::Sending)
        .await
    {
        warn!(%error, "failed to save message status");
    }
}

async fn send(
    manager: &mut RegisteredManager,
    db: &Db,
    events: &mut Events,
    thread: Thread,
    body: String,
    timestamp: u64,
) {
    let status = match send_text(manager, &thread, body, timestamp).await {
        Ok(()) => data::Status::Sent,
        Err(error) => {
            error!(%error, "failed to send message");
            data::Status::Failed
        }
    };
    if let Err(error) = db.set_send_state(timestamp, &thread, status).await {
        warn!(%error, "failed to save message status");
    }
    emit(
        events,
        Event::MessageStatus {
            timestamps: vec![timestamp],
            status,
        },
    )
    .await;
}

async fn send_text(
    manager: &mut RegisteredManager,
    thread: &Thread,
    body: String,
    timestamp: u64,
) -> Result<(), Error> {
    let message = text_message(thread, body, timestamp);
    match thread {
        Thread::Contact(contact) => {
            manager.send_message(contact, message, timestamp).await?;
        }
        Thread::Group(master_key) => {
            manager
                .send_message_to_group(master_key, message, timestamp)
                .await?;
        }
    }
    Ok(())
}

async fn load_history(
    manager: &RegisteredManager,
    db: &Db,
    thread: &Thread,
    aci: Uuid,
    before: Option<u64>,
) -> Result<(Vec<data::Message>, bool), Error> {
    let page = db.page(thread, before, super::command::PAGE).await?;
    let more = page.more;
    let mut messages = data::project(page.rows);

    let own: Vec<u64> = messages
        .iter()
        .filter(|message| message.sender() == aci)
        .map(|message| message.timestamp())
        .collect();
    let stored = db.statuses(&own, recipients(manager, thread).await).await?;
    for message in messages
        .iter_mut()
        .filter(|message| message.sender() == aci)
    {
        message.status = Some(
            stored
                .get(&message.timestamp())
                .copied()
                .unwrap_or(data::Status::Sent),
        );
    }
    Ok((messages, more))
}

/// How many receipts a message needs before it counts as delivered or read.
/// Group membership includes us, who never receipts our own message.
async fn recipients(manager: &RegisteredManager, thread: &Thread) -> usize {
    match thread {
        Thread::Contact(_) => 1,
        Thread::Group(master_key) => match manager.store().group(*master_key).await {
            Ok(Some(group)) => group.members.len().saturating_sub(1).max(1),
            _ => 1,
        },
    }
}

async fn fetch_avatars(mut manager: RegisteredManager, mut events: Events) {
    let contacts = match manager.store().contacts().await {
        Ok(contacts) => contacts.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(error) => {
            warn!(%error, "failed to list contacts for avatars");
            Vec::new()
        }
    };
    for contact in contacts {
        let thread = Thread::Contact(ContactId::Aci(contact.uuid));
        let bytes = match &contact.avatar {
            Some(avatar) => Some(avatar.reader.to_vec()),
            None => fetch_profile_avatar(&mut manager, &contact).await,
        };
        if let Some(bytes) = bytes
            && !bytes.is_empty()
        {
            emit(&mut events, Event::Avatar { thread, bytes }).await;
        }
    }

    let groups = match manager.store().groups().await {
        Ok(groups) => groups.filter_map(Result::ok).collect::<Vec<_>>(),
        Err(error) => {
            warn!(%error, "failed to list groups for avatars");
            return;
        }
    };
    for (master_key, group) in groups {
        if group.avatar.is_empty() {
            continue;
        }
        let context = GroupContextV2 {
            master_key: Some(master_key.to_vec()),
            revision: Some(group.revision),
            ..Default::default()
        };
        match manager.retrieve_group_avatar(context).await {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                emit(
                    &mut events,
                    Event::Avatar {
                        thread: Thread::Group(master_key),
                        bytes,
                    },
                )
                .await;
            }
            Ok(_) => {}
            Err(error) => warn!(%error, title = group.title, "failed to fetch group avatar"),
        }
    }
}

async fn fetch_previews(db: Db, mut events: Events) {
    let threads = match db.previews().await {
        Ok(threads) => threads,
        Err(error) => {
            warn!(%error, "failed to load thread previews");
            return;
        }
    };
    for (thread, rows) in threads {
        if let Some(message) = data::project(rows).pop() {
            emit(&mut events, Event::Preview { thread, message }).await;
        }
    }
}

async fn fetch_profile_avatar(
    manager: &mut RegisteredManager,
    contact: &presage::model::contacts::Contact,
) -> Option<Vec<u8>> {
    let key: [u8; 32] = contact.profile_key.as_slice().try_into().ok()?;
    match manager
        .retrieve_profile_avatar_by_uuid(contact.uuid, ProfileKey::create(key))
        .await
    {
        Ok(avatar) => avatar,
        Err(error) => {
            warn!(%error, uuid = %contact.uuid, "failed to fetch profile avatar");
            None
        }
    }
}

async fn send_contacts(manager: &RegisteredManager, events: &mut Events) -> Result<(), Error> {
    let store = manager.store();
    let contacts = store
        .contacts()
        .await?
        .filter_map(Result::ok)
        .map(Contact::from)
        .collect();
    let groups = store
        .groups()
        .await?
        .filter_map(Result::ok)
        .map(|(master_key, group)| Group {
            master_key,
            title: group.title,
        })
        .collect();
    emit(events, Event::Contacts { contacts, groups }).await;
    Ok(())
}
