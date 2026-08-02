use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use futures::channel::{mpsc, oneshot};
use futures::{SinkExt, StreamExt, future, pin_mut};
use presage::Manager;
use presage::libsignal_service::configuration::SignalServers;
use presage::libsignal_service::content::{ContentBody, DataMessage, GroupContextV2};
use presage::libsignal_service::proto::{AttachmentPointer, receipt_message};
use presage::libsignal_service::protocol::{Aci, ServiceId};
use presage::libsignal_service::sender::AttachmentSpec;
use presage::libsignal_service::zkgroup::profiles::ProfileKey;
use presage::manager::Registered;
use presage::model::messages::Received;
use presage::store::{ContentExt, ContentsStore, StateStore, Store};
use presage_store_sqlite::SqliteStore;
use tokio::sync::{Notify, Semaphore};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use super::cache::Cache;
use super::db::Db;
use super::{Command, Error, Event, outgoing, store};
use petunia_config as config;
use petunia_data as data;
use petunia_data::attachment;
use petunia_data::{Connection, Contact, ContactId, Fragment, Group, Thread};

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

/// Ends a session: the command loop exits either for good (the app is
/// shutting down) or to link a new account after a log out.
enum Ended {
    Shutdown,
    Relink,
}

async fn serve(mut commands: UnboundedReceiver<Command>, mut events: Events) -> Result<(), Error> {
    let db = Db::open().await?;
    let media_policy = config::load().config.media;
    let cache = Cache::new(media_policy.cache_limit);
    let limiter = Arc::new(Semaphore::new(DOWNLOADS));

    loop {
        match session(&mut commands, &mut events, &db, &cache, &limiter).await? {
            Ended::Shutdown => return Ok(()),
            Ended::Relink => continue,
        }
    }
}

async fn session(
    commands: &mut UnboundedReceiver<Command>,
    events: &mut Events,
    db: &Db,
    cache: &Cache,
    limiter: &Arc<Semaphore>,
) -> Result<Ended, Error> {
    let store = store::open().await?;
    let media_policy = config::load().config.media;

    match cache.prune().await {
        Ok(pruned) if pruned.freed == 0 => {}
        Ok(pruned) => {
            info!(
                freed = pruned.freed,
                dropped = pruned.digests.len(),
                "pruned cached media"
            );
            if let Err(error) = db.forget_blobs(&pruned.digests).await {
                warn!(%error, "failed to forget pruned blobs");
            }
        }
        Err(error) => warn!(%error, "failed to prune the media cache"),
    }
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
    let phone_number = manager.registration_data().phone_number.to_string();
    info!(%aci, "signal manager ready");
    emit(events, Event::Linked { aci, phone_number }).await;
    if let Err(error) = send_contacts(&manager, events).await {
        warn!(%error, "failed to load contacts");
    }
    // Before anything reaches the network. Every name learned on any previous
    // run is on screen by the time the window is, and the fetches below are then
    // a refresh rather than the only source there has ever been.
    send_names(db, events).await;

    // The message stream goes up *before* anything else touches the network, and
    // the order is not a preference.
    //
    // presage keeps one authenticated websocket and hands it out; `receive_messages`
    // is the one caller that will not share, because a socket already carrying
    // requests cannot also carry the stream. So a socket opened by a profile crawl
    // is a socket the stream refuses, and the stream opens a second one — two
    // connections authenticated as the same device, which is precisely what Signal
    // answers with `4409 Connected elsewhere`. Whichever loses the race is closed
    // by the server, its owner opens another, and that one closes the first: the
    // reconnect-every-five-seconds loop, with nothing but petunia at either end of
    // it. Starting the stream first means the crawls join the socket it already
    // holds.
    let (prepared_tx, mut prepared_rx) = unbounded_channel();
    let media = Media {
        manager: manager.clone(),
        cache: cache.clone(),
        db: db.clone(),
        events: events.clone(),
        limiter: limiter.clone(),
        prepared: prepared_tx,
        policy: media_policy,
    };
    let (queue_tx, mut queue_rx) = oneshot::channel();
    let wake = Arc::new(Notify::new());
    let receive_task = tokio::task::spawn_local(receive(
        manager.clone(),
        db.clone(),
        cache.clone(),
        media.clone(),
        events.clone(),
        queue_tx,
        wake.clone(),
    ));
    let sleep_watch = tokio::task::spawn_local(watch_for_sleep(wake));

    tokio::task::spawn_local(show_cached_avatars(manager.clone(), cache.clone(), events.clone()));
    tokio::task::spawn_local(refresh_profiles(
        manager.clone(),
        db.clone(),
        cache.clone(),
        events.clone(),
    ));
    tokio::task::spawn_local(fetch_previews(db.clone(), aci, events.clone()));
    tokio::task::spawn_local(refresh_contacts(manager.clone(), db.clone(), events.clone()));
    tokio::task::spawn_local(refresh_username(manager.clone(), events.clone()));
    send_sticker_packs(&manager, cache, events).await;
    // Off the loop: a pack is a CDN round trip per sticker, and the packs that
    // need one are exactly the packs nothing can be drawn of anyway.
    tokio::task::spawn_local(repair_sticker_packs(
        manager.clone(),
        cache.clone(),
        events.clone(),
    ));
    match db.flags().await {
        Ok(flags) if flags.is_empty() => {}
        Ok(flags) => emit(events, Event::Flags(flags)).await,
        Err(error) => warn!(%error, "failed to load thread flags"),
    }
    if freshly_linked
        && let Err(error) = manager.request_contacts().await
    {
        warn!(%error, "failed to request contact sync");
    }

    let mut queue_drained = false;
    let mut pending = Vec::new();
    // What has already been read is already counting down, so the first sweep
    // happens before the loop does: a conversation left open overnight should
    // not still be holding yesterday's messages when the window comes back.
    report_expire_timers(db, events).await;
    sweep_expired(db, events).await;
    let mut sweeping = tokio::time::interval(SWEEP);

    loop {
        tokio::select! {
            _ = sweeping.tick() => sweep_expired(db, events).await,
            _ = &mut queue_rx, if !queue_drained => {
                queue_drained = true;
                info!(pending = pending.len(), "message queue drained");
                for outgoing in pending.drain(..) {
                    send(&mut manager, db, events, outgoing).await;
                }
            }
            Some(outgoing) = prepared_rx.recv() => {
                queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
            }
            command = commands.recv() => match command {
                Some(Command::SendText { thread, body, ranges, quote, timestamp }) => {
                    let mut message = outgoing::text(&thread, body, &ranges, timestamp);
                    if let Some(quoted) = quote {
                        message = outgoing::replying_to(
                            message,
                            outgoing::quote(&quoted.id, &quoted.body, &quoted.ranges),
                        );
                    }
                    let message = expiring(db, &thread, message).await;
                    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
                    let outgoing = Prepared::tracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::React { thread, target, emoji, remove, timestamp }) => {
                    let message = outgoing::reaction(&thread, &target, emoji, remove, timestamp);
                    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
                    let outgoing = Prepared::untracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::DeleteMessage { thread, target, timestamp }) => {
                    let message = outgoing::delete(&thread, target, timestamp);
                    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
                    let outgoing = Prepared::untracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::DeleteForMe { thread, target }) => {
                    forget(&mut manager, db, events, &thread, target).await;
                }
                Some(Command::SetBlocked { contact, blocked }) => {
                    let service_id = ServiceId::from(Aci::from(contact));
                    match manager.set_contact_blocked(service_id, blocked).await {
                        Ok(()) => {
                            emit(events, Event::Blocked { uuid: contact, blocked }).await;
                        }
                        Err(error) => {
                            warn!(%error, blocked, "failed to change somebody's block state");
                            emit(events, Event::Error(match blocked {
                                true => format!("could not block them: {error}"),
                                false => format!("could not unblock them: {error}"),
                            }))
                            .await;
                        }
                    }
                }
                Some(Command::EditMessage { thread, target, body, ranges, timestamp }) => {
                    let message = outgoing::edit(&thread, target, body, &ranges, timestamp);
                    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
                    // Reports against the original: the edit replaces that
                    // bubble, so its status is what the UI shows.
                    let outgoing = Prepared::tracked(thread, message, timestamp)
                        .reporting(target);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::SendAttachments {
                    thread,
                    body,
                    ranges,
                    paths,
                    quote,
                    timestamp,
                    voice,
                }) => {
                    tokio::task::spawn_local(upload(
                        media.clone(),
                        thread,
                        Composed { body, ranges, quote, voice },
                        paths,
                        timestamp,
                    ));
                }
                Some(Command::LoadThread { thread, before }) => {
                    match load_history(&manager, db, cache, &thread, aci, before).await {
                        Ok(loaded) => {
                            tokio::task::spawn_local(download_all(
                                media.clone(),
                                thread.clone(),
                                loaded.pointers,
                                None,
                            ));
                            emit(events, Event::History {
                                thread,
                                messages: loaded.messages,
                                more: loaded.more,
                                covered: loaded.covered,
                                older: before.is_some(),
                            })
                            .await;
                        }
                        Err(error) => {
                            error!(%error, "failed to load message history");
                            emit(
                                events,
                                Event::Error(format!("failed to load history: {error}")),
                            )
                            .await;
                        }
                    }
                }
                Some(Command::SetFlags { thread, flags }) => {
                    if let Err(error) = db.set_flags(&thread, &flags).await {
                        warn!(%error, "failed to record thread flags");
                    }
                }
                Some(Command::DeleteThread { thread }) => {
                    // The list has already dropped it, so a failure here is
                    // reported rather than silently leaving the two disagreeing
                    // until the next restart brings the conversation back.
                    match db.delete_thread(&thread).await {
                        Ok(messages) => info!(messages, "deleted a conversation"),
                        Err(error) => {
                            error!(%error, "failed to delete a conversation");
                            emit(events, Event::Error(format!(
                                "could not delete that conversation: {error}"
                            )))
                            .await;
                        }
                    }
                }
                // Spawned rather than awaited here: it reads every row in the
                // store, and the command loop is what sends, receipts and
                // reconnects run through.
                Some(Command::CountMessages) => {
                    let aci = manager.registration_data().service_ids.aci;
                    let db = db.clone();
                    let mut events = events.clone();
                    tokio::spawn(async move {
                        match db.tallies(aci).await {
                            Ok(tallies) => emit(&mut events, Event::Counts(tallies)).await,
                            Err(error) => warn!(%error, "could not count the messages"),
                        }
                    });
                }
                Some(Command::Search { query, within }) => {
                    match db.search(&query, within.as_ref()).await {
                        Ok(hits) => emit(events, Event::Found { query, hits }).await,
                        Err(error) => warn!(%error, "search failed"),
                    }
                }
                Some(Command::MarkRead { thread, messages }) => {
                    mark_read(&mut manager, db, events, &thread, &messages).await;
                }
                Some(Command::SetExpireTimer {
                    thread,
                    seconds,
                    timestamp,
                }) => {
                    let update = outgoing::expire_timer(&thread, seconds, timestamp);
                    // Recorded before it is sent, and whatever the network
                    // says: this device has been told what the timer is, and a
                    // setting that only takes effect once the server agrees is
                    // a setting that quietly does not.
                    if let Err(error) = db.set_expire_timer(&thread, seconds).await {
                        warn!(%error, "failed to record a disappearing-message timer");
                    }
                    save_outgoing(&manager, &thread, update.clone(), timestamp).await;
                    if let Err(error) =
                        send_message(&mut manager, &thread, update.into(), timestamp).await
                    {
                        warn!(%error, "failed to tell the conversation about the timer");
                    }
                    report_expire_timers(db, events).await;
                }
                Some(Command::StartExpiry { thread, messages }) => {
                    let at = now();
                    for (target, seconds) in messages {
                        let due = at + u64::from(seconds) * 1_000;
                        if let Err(error) = db.start_expiry(&thread, target, due).await {
                            warn!(%error, "failed to start a message's clock");
                        }
                    }
                }
                Some(Command::Typing { thread, started }) => {
                    let timestamp = now();
                    let typing = outgoing::typing(&thread, started, timestamp);
                    // Not saved to the store and not tracked: a typing indicator
                    // is not part of the conversation.
                    if let Err(error) =
                        send_message(&mut manager, &thread, typing.into(), timestamp).await
                    {
                        debug!(%error, "failed to send a typing indicator");
                    }
                }
                Some(Command::SendSticker { thread, pack_id, key, sticker_id, emoji, path, timestamp }) => {
                    tokio::task::spawn_local(upload_sticker(
                        media.clone(),
                        thread,
                        Chosen { pack_id, key, sticker_id, emoji, path },
                        timestamp,
                    ));
                }
                Some(Command::InstallStickerPack { pack_id, key }) => {
                    match manager.install_sticker_pack(&pack_id, &key).await {
                        Ok(()) => {
                            info!("installed a sticker pack");
                            send_sticker_packs(&manager, cache, events).await;
                            match db.flags().await {
                                Ok(flags) if flags.is_empty() => {}
                                Ok(flags) => emit(events, Event::Flags(flags)).await,
                                Err(error) => warn!(%error, "failed to load thread flags"),
                            }
                        }
                        Err(error) => {
                            warn!(%error, "failed to install a sticker pack");
                            emit(events, Event::Error(format!(
                                "could not add that sticker pack: {error}"
                            )))
                            .await;
                        }
                    }
                }
                Some(Command::PreviewStickerPack { pack_id, key }) => {
                    // Off the loop: reading a pack is a CDN fetch per sticker,
                    // and awaited here it stopped everything else -- no messages
                    // sent, no receipts, nothing drawn -- for as long as it took.
                    tokio::task::spawn_local(read_pack(
                        manager.clone(),
                        cache.clone(),
                        events.clone(),
                        pack_id,
                        key,
                    ));
                }
                Some(Command::DownloadAttachment { thread, timestamp, id }) => {
                    match db.row(&thread, timestamp).await {
                        Ok(Some(envelope)) => {
                            tokio::task::spawn_local(download_all(
                                media.clone(),
                                thread,
                                data::pointers(&envelope),
                                Some(id),
                            ));
                        }
                        // The UI is already showing this as downloading, so a
                        // failure here has to be reported or it never settles.
                        Ok(None) => {
                            warn!(timestamp, "no stored row for the requested attachment");
                            fail(events, &thread, id, "message is no longer stored".into())
                                .await;
                        }
                        Err(error) => {
                            warn!(%error, "failed to read a row for download");
                            fail(events, &thread, id, error.to_string()).await;
                        }
                    }
                }
                Some(Command::SetNickname { contact, name, note }) => {
                    let service_id = presage::libsignal_service::protocol::ServiceId::Aci(
                        presage::libsignal_service::protocol::Aci::from(contact),
                    );
                    let (given, family) = match &name {
                        Some(name) => match name.split_once(' ') {
                            Some((given, family)) => (Some(given.to_string()), Some(family.to_string())),
                            None => (Some(name.clone()), None),
                        },
                        None => (None, None),
                    };
                    match manager
                        .set_contact_nickname(service_id, given, family, note.clone())
                        .await
                    {
                        Ok(()) => {
                            if let Err(error) =
                                db.set_nickname(contact, name.as_deref(), note.as_deref()).await
                            {
                                warn!(%error, "failed to store a nickname");
                            }
                            emit(events, Event::Nickname { uuid: contact, name, note }).await;
                        }
                        Err(error) => {
                            warn!(%error, "failed to set a contact nickname");
                            emit(events, Event::Error(format!(
                                "could not save that nickname: {error}"
                            )))
                            .await;
                        }
                    }
                }
                Some(Command::SetAvatar { bytes }) => {
                    match manager.retrieve_profile().await {
                        Ok(profile) => {
                            let name = profile.name.unwrap_or_else(|| {
                                presage::libsignal_service::profile_name::ProfileName {
                                    given_name: String::new(),
                                    family_name: None,
                                }
                            });
                            match manager
                                .update_profile_with_avatar(
                                    name,
                                    profile.about,
                                    profile.about_emoji,
                                    Some(bytes.clone()),
                                )
                                .await
                            {
                                Ok(()) => {
                                    let thread = Thread::Contact(ContactId::Aci(aci));
                                    match cache.put_avatar(&thread, &bytes).await {
                                        Ok(path) => {
                                            emit(events, Event::AvatarUpdated(path)).await
                                        }
                                        Err(error) => warn!(%error, "failed to cache the new avatar"),
                                    }
                                }
                                Err(error) => {
                                    warn!(%error, "failed to update the profile avatar");
                                    emit(events, Event::Error(format!(
                                        "could not update your profile picture: {error}"
                                    )))
                                    .await;
                                }
                            }
                        }
                        Err(error) => {
                            warn!(%error, "failed to fetch the current profile");
                            emit(events, Event::Error(format!(
                                "could not update your profile picture: {error}"
                            )))
                            .await;
                        }
                    }
                }
                Some(Command::SendPoll { thread, question, options, allow_multiple, timestamp }) => {
                    let message = outgoing::poll(&thread, question, options, allow_multiple, timestamp);
                    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
                    let outgoing = Prepared::tracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::VotePoll { thread, target, option_indexes, count, timestamp }) => {
                    let message = outgoing::poll_vote(&thread, &target, option_indexes, count, timestamp);
                    let outgoing = Prepared::untracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::TerminatePoll { thread, target, timestamp }) => {
                    let message = outgoing::poll_terminate(&thread, target, timestamp);
                    let outgoing = Prepared::untracked(thread, message, timestamp);
                    queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                }
                Some(Command::LogOut) => {
                    receive_task.abort();
                    sleep_watch.abort();
                    if let Err(error) = manager.unlink_self().await {
                        warn!(%error, "failed to unlink this device from the server");
                    }
                    let mut registration = manager.store().clone();
                    if let Err(error) = registration.clear().await {
                        error!(%error, "failed to clear the local account store");
                        emit(events, Event::Error(format!("could not log out cleanly: {error}"))).await;
                    }
                    emit(events, Event::LoggedOut).await;
                    return Ok(Ended::Relink);
                }
                Some(Command::SetUsername { nickname }) => {
                    match manager.set_username(&nickname).await {
                        Ok((username, link)) => {
                            emit(events, Event::Username(Some((username.to_string(), link.to_string())))).await;
                        }
                        Err(error) => {
                            warn!(%error, "failed to set username");
                            emit(events, Event::Error(format!("could not set that username: {error}"))).await;
                        }
                    }
                }
                Some(Command::DeleteUsername) => {
                    match manager.delete_username().await {
                        Ok(()) => emit(events, Event::Username(None)).await,
                        Err(error) => {
                            warn!(%error, "failed to delete username");
                            emit(events, Event::Error(format!("could not remove the username: {error}"))).await;
                        }
                    }
                }
                Some(Command::LookUp { query }) => {
                    let found = look_up(&mut manager, &query).await;
                    emit(events, Event::LookedUp { query, found }).await;
                }
                Some(Command::CreateGroup { title, members, timestamp }) => {
                    match manager.create_group(&title, &members).await {
                        Ok(master_key) => {
                            // Contacts and groups are re-read whole rather than
                            // patched: presage has just written the group into
                            // its own store, and one statement is cheaper than
                            // a second representation of what a group is.
                            if let Err(error) = send_contacts(&manager, events).await {
                                warn!(%error, "failed to reload groups after creating one");
                            }
                            let thread = Thread::Group(master_key);
                            let message = outgoing::group_update(&thread, timestamp);
                            let outgoing = Prepared::untracked(thread.clone(), message, timestamp);
                            queue(&mut manager, db, events, &mut pending, queue_drained, outgoing).await;
                            emit(events, Event::GroupCreated { thread }).await;
                        }
                        Err(error) => {
                            warn!(%error, "failed to create a group");
                            emit(events, Event::Error(format!("could not create the group: {error}"))).await;
                        }
                    }
                }
                None => {
                    receive_task.abort();
                    sleep_watch.abort();
                    return Ok(Ended::Shutdown);
                }
            },
        }
    }
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
    cache: Cache,
    media: Media,
    mut events: Events,
    queue_tx: oneshot::Sender<()>,
    wake: Arc<Notify>,
) {
    let mut queue_signal = Some(queue_tx);
    let mut wait = RECONNECT_MIN;
    loop {
        // Three states rather than two: opening a socket is not the same as
        // waiting to try again, and a label that says "Reconnecting" for both
        // spends the whole of a backoff claiming something is happening.
        emit(&mut events, Event::Connection(Connection::Connecting)).await;
        let started = tokio::time::Instant::now();
        let asked = match receive_once(
            &mut manager,
            &db,
            &cache,
            &media,
            &mut events,
            &mut queue_signal,
            &wake,
        )
        .await
        {
            Ok(Stopped::Asked) => true,
            Ok(Stopped::Ended) => {
                warn!(?wait, "message stream ended, reconnecting");
                false
            }
            Err(error) => {
                error!(%error, "failed to receive messages");
                emit(
                    &mut events,
                    Event::Error(format!("failed to receive messages: {error}")),
                )
                .await;
                false
            }
        };

        // Asked for, which means something outside knows the old socket is dead
        // -- the machine has been asleep. There is nothing to back off from and
        // nothing to wait for.
        if asked {
            wait = RECONNECT_MIN;
            continue;
        }

        emit(&mut events, Event::Connection(Connection::Reconnecting)).await;
        tokio::time::sleep(wait).await;

        // A stream that ran for a while and then dropped is an ordinary
        // reconnect, and the next one should be immediate. One that dies as fast
        // as it is made is a fight -- with another client, or with a second
        // connection of our own -- and reconnecting on a fixed interval keeps a
        // fight going at that interval forever. Backing off hands whichever side
        // is not ours the connection and stops asking the server for it several
        // times a minute.
        wait = match started.elapsed() >= SETTLED {
            true => RECONNECT_MIN,
            false => (wait * 2).min(RECONNECT_MAX),
        };
    }
}

/// Why a message stream stopped, which decides whether to wait before opening
/// another.
enum Stopped {
    /// The server, the network or presage ended it.
    Ended,
    /// Something here decided the socket was no longer worth anything.
    Asked,
}

/// Notices that the machine has been asleep, and throws the socket away when it
/// has.
///
/// A websocket whose peer stopped listening while the lid was shut does not
/// fail: it sits there apparently open until a keep-alive goes unanswered, which
/// is a minute or two in which nothing arrives and everything on screen is
/// stale. Nothing in the stream says the machine slept and there is no portable
/// notification to subscribe to -- but the two clocks disagree about it, since
/// `Instant` does not advance while the machine is asleep and the wall clock
/// does. A gap between them is a sleep, and a sleep is a reason to open a new
/// socket rather than wait to find out that the old one is dead.
async fn watch_for_sleep(wake: Arc<Notify>) {
    /// How often the two clocks are compared.
    const TICK: Duration = Duration::from_secs(10);
    /// How far apart they may drift before it counts as a sleep rather than as a
    /// busy machine being late to a timer.
    const GAP: Duration = Duration::from_secs(5);

    loop {
        let monotonic = tokio::time::Instant::now();
        let wall = std::time::SystemTime::now();
        tokio::time::sleep(TICK).await;

        let slept = std::time::SystemTime::now()
            .duration_since(wall)
            .unwrap_or(TICK)
            .saturating_sub(monotonic.elapsed());
        if slept > GAP {
            info!(?slept, "the machine was asleep; reopening the connection");
            wake.notify_one();
        }
    }
}

/// How long to wait before reconnecting, and how far that grows while the stream
/// keeps failing.
/// How often expired messages are looked for. Half a minute is well inside the
/// shortest timer Signal offers, and a query against an index costs nothing
/// when there is nothing to find.
const SWEEP: Duration = Duration::from_secs(30);

const RECONNECT_MIN: Duration = Duration::from_secs(5);
const RECONNECT_MAX: Duration = Duration::from_secs(120);

/// How long a stream has to have lasted to count as having worked.
const SETTLED: Duration = Duration::from_secs(60);

async fn receive_once(
    manager: &mut RegisteredManager,
    db: &Db,
    cache: &Cache,
    media: &Media,
    events: &mut Events,
    queue_signal: &mut Option<oneshot::Sender<()>>,
    wake: &Notify,
) -> Result<Stopped, Error> {
    let messages = manager.receive_messages().await?;
    pin_mut!(messages);
    info!("message stream started");
    emit(events, Event::Connection(Connection::Connected)).await;

    // Delivery receipts owed but not yet sent, by sender. See `owe`.
    let mut owed: HashMap<Uuid, Vec<u64>> = HashMap::new();
    // Master keys by the group identifier derived from them. See `typing_thread`.
    let mut group_ids: HashMap<Vec<u8>, [u8; 32]> = HashMap::new();

    loop {
        // Biased, so that a socket already known to be dead is dropped rather
        // than read: whatever is buffered behind it arrives again over the next
        // one, and reading it here only delays that.
        let received = tokio::select! {
            biased;
            () = wake.notified() => {
                deliver_all(manager, &mut owed);
                return Ok(Stopped::Asked);
            }
            received = messages.next() => match received {
                Some(received) => received,
                None => break,
            },
        };

        match received {
            Received::QueueEmpty => {
                debug!("message queue empty");
                deliver_all(manager, &mut owed);
                if let Some(signal) = queue_signal.take() {
                    let _ = signal.send(());
                }
            }
            Received::Contacts => {
                info!("contacts synced");
                if let Err(error) = send_contacts(manager, events).await {
                    warn!(%error, "failed to load synced contacts");
                }
                tokio::task::spawn_local(refresh_profiles(
                    manager.clone(),
                    db.clone(),
                    cache.clone(),
                    events.clone(),
                ));
                let aci = manager.registration_data().service_ids.aci;
                tokio::task::spawn_local(fetch_previews(db.clone(), aci, events.clone()));
            }
            Received::Content(content) => {
                debug!(timestamp = content.timestamp(), "received content");

                // The server asks for this, and presage never answers, so the
                // sender would otherwise never see a second tick.
                if content.metadata.needs_receipt {
                    let sender = content.metadata.sender.raw_uuid();
                    let sent_at = content.metadata.timestamp.timestamp_millis() as u64;
                    owe(manager, &mut owed, queue_signal.is_some(), sender, sent_at);
                }

                // What we read (or viewed) on another device. presage stores
                // the sync and ignores it, so without this reading on the
                // phone never clears anything here. A view-once message read
                // there is a `viewed` mark rather than a `read` one, but both
                // clear the same unread state.
                if let ContentBody::SynchronizeMessage(sync) = &content.body
                    && (!sync.read.is_empty() || !sync.viewed.is_empty())
                {
                    let marks = sync
                        .read
                        .iter()
                        .map(|read| (read.sender_aci.as_ref(), read.timestamp()))
                        .chain(
                            sync.viewed
                                .iter()
                                .map(|viewed| (viewed.sender_aci.as_ref(), viewed.timestamp())),
                        );
                    for (sender_aci, upto) in marks {
                        let Some(sender) = sender_aci.and_then(|aci| aci.parse::<Uuid>().ok())
                        else {
                            continue;
                        };
                        let thread = Thread::Contact(ContactId::Aci(sender));
                        if let Err(error) = db.mark_read(&thread, upto).await {
                            warn!(%error, "failed to record a synced read mark");
                        }
                        emit(events, Event::Read { thread, upto }).await;
                    }
                    continue;
                }

                if let ContentBody::TypingMessage(typing) = &content.body {
                    let started = typing.action()
                        == presage::libsignal_service::proto::typing_message::Action::Started;
                    if let Some(thread) =
                        typing_thread(manager, &mut group_ids, typing, &content).await
                    {
                        emit(events, Event::Typing {
                            thread,
                            sender: content.metadata.sender.raw_uuid(),
                            started,
                        })
                        .await;
                    }
                } else if let Some((timestamps, status)) = data::receipt_from_content(&content) {
                    let recipient = content.metadata.sender.raw_uuid();
                    if let Err(error) = db.record_receipts(&timestamps, recipient, status).await {
                        warn!(%error, "failed to save receipt statuses");
                    }
                    advance_preview(db, &timestamps, status).await;
                    emit(events, Event::MessageStatus { timestamps, status }).await;
                } else if let Some((thread, mut fragment)) = data::classify(&content) {
                    if let Fragment::Message(message) | Fragment::Edit { message, .. } =
                        &mut fragment
                    {
                        hydrate(cache, std::slice::from_mut(message)).await;
                        index_bodies(db, &thread, std::slice::from_ref(message)).await;
                        // The one moment writing the sidebar's line is free: the
                        // message is decoded, projected and in hand. Everything
                        // else that ever needed it had to work it out again.
                        let aci = manager.registration_data().service_ids.aci;
                        remember_preview(db, &thread, message, aci).await;

                        // A one-to-one timer is kept nowhere a client can read
                        // it back: the update *is* the setting, so an arriving
                        // one has to be written down or the conversation forgets
                        // what it was told the moment the window closes.
                        if let data::message::Content::Update(
                            data::message::Update::ExpireTimer { seconds },
                        ) = message.content
                        {
                            if let Err(error) = db.set_expire_timer(&thread, seconds).await {
                                warn!(%error, "failed to record an arriving timer");
                            }
                            report_expire_timers(db, events).await;
                        }
                    }
                    let pointers = data::pointers(&content);
                    if !pointers.is_empty() {
                        tokio::task::spawn_local(download_all(
                            media.clone(),
                            thread.clone(),
                            pointers,
                            None,
                        ));
                    }
                    emit(events, Event::Fragment {
                        thread,
                        fragment,
                        order: content.metadata.timestamp.timestamp_millis() as u64,
                    })
                    .await;
                }
            }
        }
    }
    // The stream ended without the server ever saying the queue was empty, which
    // is a reconnect: what is still owed has to go now, since nothing will
    // deliver these messages a second time to ask again.
    deliver_all(manager, &mut owed);
    Ok(Stopped::Ended)
}

/// Which conversation somebody is typing in.
///
/// presage's `Thread::try_from` reads the group out of a *data* message and
/// answers `Contact(sender)` for everything else, a typing indicator included —
/// so every group's dots were filed under the sender's one-to-one thread, where
/// they appeared beside the wrong name in the list and never in the group at all.
///
/// A `TypingMessage` names its group by the identifier *derived* from the master
/// key, and that derivation only runs one way: the answer is to derive it for
/// every group this account is in and match. Cached for the life of the stream,
/// and rebuilt when an id is not in it, which is what a group joined since the
/// stream started looks like.
async fn typing_thread(
    manager: &RegisteredManager,
    known: &mut HashMap<Vec<u8>, [u8; 32]>,
    typing: &presage::libsignal_service::proto::TypingMessage,
    content: &presage::libsignal_service::content::Content,
) -> Option<Thread> {
    let Some(id) = typing.group_id.as_ref().filter(|id| !id.is_empty()) else {
        return Some(Thread::Contact((&content.metadata.sender).into()));
    };

    if !known.contains_key(id) {
        match manager.store().groups().await {
            Ok(groups) => {
                *known = groups
                    .filter_map(Result::ok)
                    .map(|(master_key, _)| (outgoing::group_identifier(&master_key), master_key))
                    .collect();
            }
            Err(error) => warn!(%error, "failed to list groups for a typing indicator"),
        }
    }

    match known.get(id) {
        Some(master_key) => Some(Thread::Group(*master_key)),
        // A group we are not in, which is nothing we can draw dots for.
        None => {
            debug!("typing indicator for an unknown group");
            None
        }
    }
}

/// Records a delivery receipt, and sends it when there is nothing to be gained
/// by holding it.
///
/// A receipt is a sealed-sender message, so it is a network round trip. Awaited
/// in the receive loop it was one round trip *per message before the next one
/// was read*, and a backlog then arrived at the speed of the receipts rather
/// than of the stream — which is the whole of "petunia syncs slower than
/// Signal". One receipt carries any number of timestamps, so behind a backlog
/// they are collected and sent per sender when it drains; live there is a single
/// message to answer and it goes at once, off the loop rather than in it.
fn owe(
    manager: &RegisteredManager,
    owed: &mut HashMap<Uuid, Vec<u64>>,
    backlog: bool,
    sender: Uuid,
    timestamp: u64,
) {
    /// A receipt is a protobuf of timestamps, so the batch is bounded by what
    /// fits in a message rather than by taste.
    const BATCH: usize = 512;

    if !backlog {
        deliver(manager, sender, vec![timestamp]);
        return;
    }

    let batch = owed.entry(sender).or_default();
    batch.push(timestamp);
    if batch.len() >= BATCH {
        let full = std::mem::take(batch);
        owed.remove(&sender);
        deliver(manager, sender, full);
    }
}

fn deliver_all(manager: &RegisteredManager, owed: &mut HashMap<Uuid, Vec<u64>>) {
    for (sender, timestamps) in owed.drain() {
        deliver(manager, sender, timestamps);
    }
}

/// Writes the row presage would have written had this come off the wire, so the
/// message survives a restart before it is sent.
async fn save_outgoing(
    manager: &RegisteredManager,
    thread: &Thread,
    message: impl Into<ContentBody>,
    timestamp: u64,
) {
    let content = outgoing::envelope(
        manager.registration_data().service_ids.aci,
        manager.device_id(),
        thread,
        message,
        timestamp,
    );

    if let Err(error) = manager.store().save_message(&thread.into(), content).await {
        warn!(%error, "failed to save outgoing message");
    }
}

async fn send(manager: &mut RegisteredManager, db: &Db, events: &mut Events, outgoing: Prepared) {
    let Prepared {
        thread,
        body,
        sent_at,
        reports,
    } = outgoing;

    let status = match send_message(manager, &thread, body, sent_at).await {
        Ok(()) => data::Status::Sent,
        Err(error) => {
            error!(%error, "failed to send message");
            data::Status::Failed
        }
    };
    let Some(timestamp) = reports else {
        return;
    };
    if let Err(error) = db.set_send_state(timestamp, &thread, status).await {
        warn!(%error, "failed to save message status");
    }
    advance_preview(db, &[timestamp], status).await;
    emit(
        events,
        Event::MessageStatus {
            timestamps: vec![timestamp],
            status,
        },
    )
    .await;
}

async fn send_message(
    manager: &mut RegisteredManager,
    thread: &Thread,
    message: ContentBody,
    timestamp: u64,
) -> Result<(), Error> {
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

struct Loaded {
    messages: Vec<data::Message>,
    more: bool,
    /// The oldest row the page reached, message or not.
    covered: Option<u64>,
    pointers: Vec<data::Wanted>,
}

async fn load_history(
    manager: &RegisteredManager,
    db: &Db,
    cache: &Cache,
    thread: &Thread,
    aci: Uuid,
    before: Option<u64>,
) -> Result<Loaded, Error> {
    let page = db.page(thread, before, super::command::PAGE).await?;
    let more = page.more;
    // Every row the page reached, not only the ones that became a message: a
    // reaction, an edit and a delete are rows too, and the page behind this one
    // has to be asked for from behind them.
    let covered = page.covered;
    let pointers: Vec<_> = page.rows.iter().flat_map(data::pointers).collect();
    let mut messages = data::project(page.rows);
    hydrate(cache, &mut messages).await;

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
    index_bodies(db, thread, &messages).await;

    Ok(Loaded {
        messages,
        more,
        covered,
        pointers,
    })
}

/// Records what a page of messages said, so a search can find them. Free at this
/// point: the rows are decoded and in hand.
async fn index_bodies(db: &Db, thread: &Thread, messages: &[data::Message]) {
    let bodies: Vec<_> = messages
        .iter()
        .filter_map(|message| {
            message
                .text()
                .filter(|text| !text.trim().is_empty())
                .map(|text| (message.timestamp(), message.sender(), text.to_owned()))
        })
        .collect();

    if let Err(error) = db.index_bodies(thread, &bodies).await {
        warn!(%error, "failed to index message bodies");
    }
}

/// Keeps the line the sidebar will draw for this thread. Older lines are refused
/// by the store, so this can be called for any message without asking whether it
/// is the newest one.
/// How far one of our own messages got, as far as anything on disk can say.
///
/// A group is counted as having more recipients than have reported, so it reads
/// as sent rather than as read: the member list lives with the manager and this
/// runs before it, and claiming delivery on one person's receipt is the one answer
/// that would be a lie. Receipts arriving afterwards raise it in the ordinary way.
async fn stored_status(db: &Db, thread: &Thread, timestamp: u64) -> data::Status {
    let recipients = match thread {
        Thread::Contact(_) => 1,
        Thread::Group(_) => usize::MAX,
    };
    db.statuses(&[timestamp], recipients)
        .await
        .unwrap_or_default()
        .get(&timestamp)
        .copied()
        .unwrap_or(data::Status::Sent)
}

/// Raises the status on the sidebar's line, when the line is of the message the
/// status is about. Written beside the receipt it comes from, because the list is
/// not rebuilt from the histories at startup: a thread nobody has opened has no
/// history for a tick to be read out of.
async fn advance_preview(db: &Db, timestamps: &[u64], status: data::Status) {
    if let Err(error) = db.advance_preview(timestamps, status).await {
        warn!(%error, "failed to record a receipt against a thread preview");
    }
}

async fn remember_preview(db: &Db, thread: &Thread, message: &data::Message, aci: Uuid) {
    let mut preview = data::index::Preview::of(message);
    // A message of ours arriving here came from another of this account's
    // devices, so nothing local sent it and there is no send state to read. It
    // still gets receipts, and a status is what a receipt raises: without one the
    // row would have no mark on it and no way to grow one.
    if message.sender() == aci && preview.status.is_none() {
        preview.status = Some(stored_status(db, thread, message.timestamp()).await);
    }
    if let Err(error) = db.set_preview(thread, &preview).await {
        warn!(%error, "failed to store a thread preview");
    }
}

/// Marks anything already on disk as cached, so media that has been fetched
/// before shows up without waiting on the network.
async fn hydrate(cache: &Cache, messages: &mut [data::Message]) {
    for message in messages {
        let attachments: Vec<_> = message
            .attachment_refs()
            .map(|attached| (attached.id.clone(), wants_measuring(attached)))
            .collect();
        for (id, measure) in attachments {
            if let Some(path) = cache.attachment(&id).await {
                if measure && let Some(size) = measured(&path).await {
                    message.set_image_size(&id, size);
                }
                if let Some(poster) = cache.poster(&id).await {
                    message.set_poster(&id, poster);
                }
                message.set_blob(&id, attachment::Blob::Cached(path));
            }
        }
    }
}

/// An image whose sender did not declare its dimensions. Signal usually does,
/// but a wrong aspect ratio is visible in a way a missing one is not.
fn wants_measuring(attached: &attachment::Attachment) -> bool {
    matches!(attached.kind, attachment::Kind::Image { size: None, .. })
}

/// Generates and caches the still a video is shown as. Off the command loop, on
/// a blocking thread: pulling one frame means opening a decoder.
async fn poster(cache: &Cache, id: &attachment::Id, path: &Path) -> Option<std::path::PathBuf> {
    if let Some(existing) = cache.poster(id).await {
        return Some(existing);
    }
    let source = path.to_path_buf();
    let bytes = tokio::task::spawn_blocking(move || petunia_media::video::poster(&source))
        .await
        .ok()
        .flatten()?;

    cache
        .put_poster(id, &bytes)
        .await
        .inspect_err(|error| warn!(%error, "failed to cache a video poster"))
        .ok()
}

/// Reads the header rather than decoding the pixels, so this is a few hundred
/// bytes off disk however large the picture is.
async fn measured(path: &Path) -> Option<attachment::Size> {
    let path = path.to_path_buf();
    tokio::task::spawn_blocking(move || attachment::dimensions(&path))
        .await
        .ok()
        .flatten()
}

/// Bounds concurrent downloads so opening a media-heavy thread does not fire a
/// page worth of requests at once.
const DOWNLOADS: usize = 3;

/// presage never persists attachment bytes and Signal's CDN expires entries after
/// a few weeks, so the cheap common cases are fetched eagerly or they become
/// unrecoverable. Anything else waits to be asked for.
fn auto_download(pointer: &AttachmentPointer, policy: &config::Media) -> bool {
    if pointer.size() > policy.auto_download_limit.saturating_mul(1024 * 1024) {
        return false;
    }
    match pointer.content_type().split('/').next().unwrap_or_default() {
        "image" => policy.auto_download_images,
        "audio" => policy.auto_download_audio,
        "video" => policy.auto_download_video,
        _ => false,
    }
}

/// Everything a background upload or download needs. Held by value because these
/// tasks outlive the command loop iteration that spawned them.
#[derive(Clone)]
struct Media {
    manager: RegisteredManager,
    cache: Cache,
    db: Db,
    events: Events,
    limiter: Arc<Semaphore>,
    prepared: UnboundedSender<Prepared>,
    policy: config::Media,
}

/// Something ready to go on the wire. Uploads produce these off the command loop
/// and hand them back, because sending needs `&mut Manager`.
struct Prepared {
    thread: Thread,
    body: ContentBody,
    /// The timestamp it goes out with, which is also its row's identity.
    sent_at: u64,
    /// The bubble whose status this reports. `None` for reactions and deletes,
    /// which show no bubble of their own -- tracking them would only litter
    /// `petunia_send` with rows nothing reads. For an edit it is the *original*
    /// timestamp, since the edit replaces that bubble.
    reports: Option<u64>,
}

impl Prepared {
    fn tracked(thread: Thread, body: impl Into<ContentBody>, sent_at: u64) -> Self {
        Self {
            thread,
            body: body.into(),
            sent_at,
            reports: Some(sent_at),
        }
    }

    fn untracked(thread: Thread, body: impl Into<ContentBody>, sent_at: u64) -> Self {
        Self {
            reports: None,
            ..Self::tracked(thread, body, sent_at)
        }
    }

    fn reporting(self, timestamp: u64) -> Self {
        Self {
            reports: Some(timestamp),
            ..self
        }
    }
}

/// Sends now, or holds until presage has drained the incoming queue -- sending
/// before that races the initial sync.
async fn queue(
    manager: &mut RegisteredManager,
    db: &Db,
    events: &mut Events,
    pending: &mut Vec<Prepared>,
    drained: bool,
    outgoing: Prepared,
) {
    // Recorded before the attempt so a crash mid-send is swept to Failed at the
    // next startup rather than looking sent.
    if let Some(timestamp) = outgoing.reports
        && let Err(error) = db
            .set_send_state(timestamp, &outgoing.thread, data::Status::Sending)
            .await
    {
        warn!(%error, "failed to save message status");
    }
    if drained {
        send(manager, db, events, outgoing).await;
    } else {
        pending.push(outgoing);
    }
}

/// Reads and uploads the files, then queues the finished message. Runs off the
/// command loop because `upload_attachments` needs only `&Manager`, so a large
/// file does not stall every other send.
/// The parts of a message that survive an upload, held together so the send path
/// does not take five positional arguments that are easy to transpose.
struct Composed {
    body: String,
    ranges: Vec<data::message::Range>,
    quote: Option<super::command::Quoted>,
    voice: bool,
}

async fn upload(
    context: Media,
    thread: Thread,
    composed: Composed,
    paths: Vec<PathBuf>,
    timestamp: u64,
) {
    let Composed {
        body,
        ranges,
        quote,
        voice,
    } = composed;
    let Media {
        manager,
        cache,
        db,
        mut events,
        prepared,
        ..
    } = context;

    let mut specs = Vec::new();
    for path in &paths {
        match tokio::fs::read(path).await {
            Ok(bytes) => specs.push((spec(path, bytes.len(), voice), bytes)),
            Err(error) => {
                error!(%error, path = %path.display(), "failed to read an attachment");
                fail_send(&mut events, timestamp).await;
                return;
            }
        }
    }

    let uploaded = match manager.upload_attachments(specs).await {
        Ok(uploaded) => uploaded,
        Err(error) => {
            error!(%error, "failed to start an attachment upload");
            fail_send(&mut events, timestamp).await;
            return;
        }
    };

    let mut pointers = Vec::new();
    for result in uploaded {
        match result {
            Ok(pointer) => pointers.push(pointer),
            Err(error) => {
                error!(%error, "failed to upload an attachment");
                fail_send(&mut events, timestamp).await;
                return;
            }
        }
    }

    for (path, pointer) in paths.iter().zip(&pointers) {
        adopt(&cache, &db, path, pointer).await;
    }

    let mut message = outgoing::message(&thread, body, &ranges, pointers, timestamp);
    if let Some(quoted) = quote {
        message = outgoing::replying_to(
            message,
            outgoing::quote(&quoted.id, &quoted.body, &quoted.ranges),
        );
    }
    let message = expiring(&db, &thread, message).await;
    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
    let _ = prepared.send(Prepared::tracked(thread, message, timestamp));
}

/// Which sticker, and where its bytes are on disk.
struct Chosen {
    pack_id: Vec<u8>,
    key: Vec<u8>,
    sticker_id: u32,
    emoji: Option<String>,
    path: PathBuf,
}

async fn upload_sticker(context: Media, thread: Thread, chosen: Chosen, timestamp: u64) {
    let Media {
        manager,
        mut events,
        prepared,
        ..
    } = context;

    let bytes = match tokio::fs::read(&chosen.path).await {
        Ok(bytes) => bytes,
        Err(error) => {
            error!(%error, "failed to read a sticker");
            fail_send(&mut events, timestamp).await;
            return;
        }
    };
    let spec = AttachmentSpec {
        content_type: attachment::content_type(&chosen.path),
        length: bytes.len(),
        ..Default::default()
    };

    let pointer = match manager.upload_attachments(vec![(spec, bytes)]).await {
        Ok(mut uploaded) => match uploaded.pop() {
            Some(Ok(pointer)) => pointer,
            _ => {
                error!("sticker upload produced no pointer");
                fail_send(&mut events, timestamp).await;
                return;
            }
        },
        Err(error) => {
            error!(%error, "failed to upload a sticker");
            fail_send(&mut events, timestamp).await;
            return;
        }
    };

    let message = outgoing::sticker(
        &thread,
        chosen.pack_id,
        chosen.key,
        chosen.sticker_id,
        chosen.emoji,
        pointer,
        timestamp,
    );
    save_outgoing(&manager, &thread, message.clone(), timestamp).await;
    let _ = prepared.send(Prepared::tracked(thread, message, timestamp));
}

/// A voice note says so on the wire. It is a flag rather than a content type --
/// Signal sends AAC and petunia sends PCM, and neither of those is what makes a
/// recording a voice note -- and it is the flag every other client reads to draw
/// a waveform and a play button instead of a paperclip.
fn spec(path: &Path, length: usize, voice_note: bool) -> AttachmentSpec {
    AttachmentSpec {
        content_type: attachment::content_type(path),
        length,
        file_name: path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned()),
        voice_note: voice_note.then_some(true),
        ..Default::default()
    }
}

/// Keeps a copy of what we just sent under the digest the stored row will carry,
/// so reloading the thread later finds it without asking the CDN for something it
/// may already have expired.
async fn adopt(cache: &Cache, db: &Db, source: &Path, pointer: &AttachmentPointer) {
    let Some(attached) = attachment::from_pointer(pointer) else {
        return;
    };
    match cache
        .adopt_attachment(&attached.id, &attached.content_type, source)
        .await
    {
        Ok(_) => {
            if let Err(error) = db
                .record_blob(&attached.id, &attached.content_type, attached.size)
                .await
            {
                warn!(%error, "failed to record a sent blob");
            }
        }
        Err(error) => warn!(%error, "failed to cache a sent attachment"),
    }
}

/// No row was saved, so nothing needs correcting in the store -- only the
/// optimistic message the UI is already showing as sending.
async fn fail_send(events: &mut Events, timestamp: u64) {
    emit(
        events,
        Event::MessageStatus {
            timestamps: vec![timestamp],
            status: data::Status::Failed,
        },
    )
    .await;
}

/// Fetches the pointers a thread needs, skipping whatever is already on disk.
/// Runs on a clone because `get_attachment` only needs `&Manager`, so downloads
/// do not block the command loop, which needs `&mut`.
async fn download_all(
    context: Media,
    thread: Thread,
    pointers: Vec<data::Wanted>,
    forced: Option<attachment::Id>,
) {
    let Media {
        manager,
        cache,
        db,
        mut events,
        limiter,
        policy,
        ..
    } = context;

    for data::Wanted {
        id,
        pointer,
        essential,
    } in pointers
    {
        let wanted = match &forced {
            Some(only) => *only == id,
            // A sticker is fetched whatever it claims to be; see `data::Wanted`.
            None => essential || auto_download(&pointer, &policy),
        };
        if !wanted {
            continue;
        }
        let content_type = pointer.content_type().to_string();
        if cache.attachment(&id).await.is_some() {
            continue;
        }

        let Ok(_permit) = limiter.acquire().await else {
            return;
        };
        emit(
            &mut events,
            Event::Attachment {
                thread: thread.clone(),
                id: id.clone(),
                blob: attachment::Blob::Downloading(None),
                measured: None,
            },
        )
        .await;

        // The declared size is the *plaintext* length and what arrives is the
        // ciphertext, which carries an IV and a MAC on top of it — so the
        // fraction is clamped rather than allowed to read 100.2%. Reported
        // every `STEP` bytes rather than every chunk: a bar is a few hundred
        // pixels wide and one event per sixty-four kilobytes of a large video
        // is a thousand repaints saying nothing new.
        let declared = pointer.size().max(1) as f64;
        let reporter = events.clone();
        let (reporting, thread_for) = (id.clone(), thread.clone());
        let mut reported = 0u64;
        let progress = move |bytes: u64| {
            const STEP: u64 = 256 * 1024;
            if bytes < reported + STEP {
                return;
            }
            reported = bytes;
            let fraction = (bytes as f64 / declared).clamp(0.0, 1.0) as f32;
            // `try_send` on an unbounded channel only fails once the receiver is
            // gone, and this is inside a synchronous closure the reader cannot
            // be awaited from.
            let _ = reporter.clone().try_send(Event::Attachment {
                thread: thread_for.clone(),
                id: reporting.clone(),
                blob: attachment::Blob::Downloading(Some(fraction)),
                measured: None,
            });
        };

        match manager.get_attachment_reporting(&pointer, progress).await {
            Ok(bytes) => match cache.put_attachment(&id, &content_type, &bytes).await {
                Ok(path) => {
                    if let Err(error) = db.record_blob(&id, &content_type, bytes.len() as u64).await
                    {
                        warn!(%error, "failed to record a cached blob");
                    }
                    let measured = match content_type.starts_with("image/") {
                        true => measured(&path).await,
                        false => None,
                    };
                    // A clip with no still is a grey rectangle, and Signal sends
                    // no thumbnail for video.
                    if content_type.starts_with("video/")
                        && let Some(poster) = poster(&cache, &id, &path).await
                    {
                        emit(&mut events, Event::Poster {
                            thread: thread.clone(),
                            id: id.clone(),
                            path: poster,
                        })
                        .await;
                    }
                    emit(
                        &mut events,
                        Event::Attachment {
                            thread: thread.clone(),
                            id,
                            blob: attachment::Blob::Cached(path),
                            measured,
                        },
                    )
                    .await;
                }
                Err(error) => {
                    warn!(%error, "failed to cache an attachment");
                    fail(&mut events, &thread, id, error.to_string()).await;
                }
            },
            Err(error) => {
                warn!(%error, "failed to download an attachment");
                fail(&mut events, &thread, id, error.to_string()).await;
            }
        }
    }
}

async fn fail(events: &mut Events, thread: &Thread, id: attachment::Id, error: String) {
    emit(
        events,
        Event::Attachment {
            thread: thread.clone(),
            id,
            blob: attachment::Blob::Failed(error),
            measured: None,
        },
    )
    .await;
}

/// Marks a message with the conversation's disappearing timer, if it has one.
///
/// On the way out rather than at the composer: the timer is a property of the
/// conversation and the worker is what remembers it, and a message sent without
/// the field set is a message that never disappears for anybody — including for
/// the person who turned the timer on.
async fn expiring(db: &Db, thread: &Thread, mut message: DataMessage) -> DataMessage {
    match db.expire_timers().await {
        Ok(timers) => {
            message.expire_timer = timers
                .iter()
                .find(|(of, _)| of == thread)
                .map(|(_, seconds)| *seconds);
        }
        Err(error) => warn!(%error, "could not read the timer for an outgoing message"),
    }
    message
}

/// Tells the window what every conversation's disappearing timer is.
async fn report_expire_timers(db: &Db, events: &mut Events) {
    match db.expire_timers().await {
        Ok(timers) => emit(events, Event::ExpireTimers(timers)).await,
        Err(error) => warn!(%error, "failed to read the disappearing-message timers"),
    }
}

/// Deletes whatever is due, and tells the window so the rows go from the
/// conversation on screen rather than only from the file under it.
///
/// Locally and only locally: an expiry is not a remote delete. Everybody else
/// has the same timer and is running the same clock, and asking them to delete
/// something they are already deleting is a message per message per recipient.
async fn sweep_expired(db: &Db, events: &mut Events) {
    let due = match db.due(now()).await {
        Ok(due) => due,
        Err(error) => {
            warn!(%error, "failed to look for expired messages");
            return;
        }
    };

    for (thread, target) in due {
        if let Err(error) = db.delete_message(&thread, target.timestamp).await {
            warn!(%error, "failed to delete an expired message");
            continue;
        }
        if let Err(error) = db.forget_expiry(&thread, target.timestamp).await {
            warn!(%error, "failed to forget an expiry");
        }
        emit(events, Event::Forgotten { thread, target }).await;
    }
}

/// Sends READ receipts to each sender and tells our own other devices, which
/// presage does neither of.
async fn mark_read(
    manager: &mut RegisteredManager,
    db: &Db,
    events: &mut Events,
    thread: &Thread,
    messages: &[(Uuid, u64)],
) {
    if messages.is_empty() {
        return;
    }

    // Written before the receipts go out: what has been read is true of this
    // device whether or not the network agrees, and it has to survive a restart.
    if let Some(newest) = messages.iter().map(|(_, timestamp)| *timestamp).max()
        && let Err(error) = db.mark_read(thread, newest).await
    {
        warn!(%error, "failed to record the read mark");
    }

    // One receipt per sender, carrying every timestamp of theirs at once.
    let mut by_sender: Vec<(Uuid, Vec<u64>)> = Vec::new();
    for (sender, timestamp) in messages {
        match by_sender.iter_mut().find(|(who, _)| who == sender) {
            Some((_, timestamps)) => timestamps.push(*timestamp),
            None => by_sender.push((*sender, vec![*timestamp])),
        }
    }

    for (sender, timestamps) in &by_sender {
        let receipt = outgoing::receipt(receipt_message::Type::Read, timestamps.clone());
        let to = ContactId::Aci(*sender);
        if let Err(error) = manager.send_message(&to, receipt, now()).await {
            warn!(%error, "failed to send a read receipt");
        }
    }

    let aci = manager.registration_data().service_ids.aci;
    let sync = outgoing::read_sync(messages);
    if let Err(error) = manager
        .send_message(&ContactId::Aci(aci), sync, now())
        .await
    {
        warn!(%error, "failed to sync read state to our other devices");
    }
    let _ = events;
}

/// "Delete for me": the row goes from this device, and the account's other
/// devices are told to drop it too.
///
/// The local delete goes first and the event is emitted whatever the sync does.
/// The reader asked for the message to be gone from in front of them; a network
/// failure is a reason the *other* devices have not caught up, not a reason to
/// leave it on screen here.
async fn forget(
    manager: &mut RegisteredManager,
    db: &Db,
    events: &mut Events,
    thread: &Thread,
    target: data::MessageId,
) {
    match db.delete_message(thread, target.timestamp).await {
        Ok(gone) => debug!(gone, ts = target.timestamp, "deleted a message for ourselves"),
        Err(error) => {
            error!(%error, "failed to delete a message locally");
            emit(events, Event::Error(format!("could not delete that message: {error}"))).await;
            return;
        }
    }
    emit(events, Event::Forgotten { thread: thread.clone(), target }).await;

    let aci = manager.registration_data().service_ids.aci;
    let sync = outgoing::delete_for_me(thread, &target);
    if let Err(error) = manager
        .send_message(&ContactId::Aci(aci), sync, now())
        .await
    {
        warn!(%error, "failed to sync a deletion to our other devices");
    }
}

/// Everybody this account has blocked, read out of Storage Service at startup.
///
/// Signal's block list is the `blocked` flag on each contact record, so it comes
/// out of the same manifest read the nicknames do -- which is why they are one
/// pass rather than two.
async fn refresh_contacts(mut manager: RegisteredManager, db: Db, mut events: Events) {
    let records = match manager.contact_records().await {
        Ok(records) => records,
        Err(error) => {
            debug!(%error, "failed to read storage service contact records");
            return;
        }
    };

    for record in records {
        let Some(uuid) = uuid_from_contact_record(&record) else {
            continue;
        };
        let name = record.nickname.as_ref().map(|name| {
            [name.given.as_str(), name.family.as_str()]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
        });
        let note = Some(record.note.clone()).filter(|note| !note.is_empty());
        if name.is_some() || note.is_some() {
            if let Err(error) = db.set_nickname(uuid, name.as_deref(), note.as_deref()).await {
                warn!(%error, %uuid, "failed to store a nickname");
            }
            emit(&mut events, Event::Nickname { uuid, name, note }).await;
        }
        // Only the blocked ones. Everybody else is the default, and saying so
        // per contact is a message per person in the address book.
        if record.blocked {
            emit(&mut events, Event::Blocked { uuid, blocked: true }).await;
        }
    }
}

/// Acknowledges receipt of a message the server asked us to confirm. presage
/// never sends these, so without it the sender never sees a second tick.
/// Spawned rather than awaited: the caller is the receive loop, and nothing it
/// does next depends on the answer. The socket is an id-matched request channel,
/// so a receipt in flight is one more request rather than a second connection.
fn deliver(manager: &RegisteredManager, sender: Uuid, timestamps: Vec<u64>) {
    let mut manager = manager.clone();
    tokio::task::spawn_local(async move {
        let receipt = outgoing::receipt(receipt_message::Type::Delivery, timestamps);
        if let Err(error) = manager
            .send_message(&ContactId::Aci(sender), receipt, now())
            .await
        {
            debug!(%error, "failed to send a delivery receipt");
        }
    });
}

fn now() -> u64 {
    chrono::Utc::now().timestamp_millis() as u64
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

/// Shows what is already on disk, at once. Nothing here touches the network:
/// the point is that no picture is missing while the refresh runs.
async fn show_cached_avatars(manager: RegisteredManager, cache: Cache, mut events: Events) {
    let mut threads = vec![Thread::Contact(ContactId::Aci(
        manager.registration_data().service_ids.aci,
    ))];
    if let Ok(contacts) = manager.store().contacts().await {
        threads.extend(
            contacts
                .filter_map(Result::ok)
                .map(|contact| Thread::Contact(ContactId::Aci(contact.uuid))),
        );
    }
    if let Ok(groups) = manager.store().groups().await {
        threads.extend(
            groups
                .filter_map(Result::ok)
                .map(|(master_key, _)| Thread::Group(master_key)),
        );
    }

    for thread in threads {
        if let Some(path) = cache.avatar(&thread).await {
            emit(&mut events, Event::Avatar { thread, path }).await;
        }
    }
}

/// Reads the username this account already has out of Storage Service, once at
/// startup.
///
/// Signal's servers only ever hold a *hash* of a username, so there is nothing
/// to ask them: the plaintext lives in the account record every linked device
/// shares. Without this, a username set on the phone was invisible here and the
/// settings panel said there was none.
async fn refresh_username(mut manager: RegisteredManager, mut events: Events) {
    match manager.username().await {
        Ok(Some((username, link))) => {
            let link = link.map(|link| link.to_string()).unwrap_or_default();
            emit(&mut events, Event::Username(Some((username.to_string(), link)))).await;
        }
        Ok(None) => debug!("this account has no username"),
        Err(error) => debug!(%error, "failed to read the account's username"),
    }
}

/// Resolves what somebody typed into the new-chat field to an account.
///
/// A username goes through the username hash lookup; a phone
/// number goes through contact discovery. Neither is tried against the other:
/// the two are told apart by shape, and a query that is neither resolves to
/// nothing rather than to two failed round trips.
async fn look_up(manager: &mut RegisteredManager, query: &str) -> Option<Contact> {
    let query = query.trim();
    let uuid: Uuid = if query.starts_with('+') {
        manager
            .discover_contacts_by_phone_number([query])
            .await
            .inspect_err(|error| debug!(%error, "phone number lookup failed"))
            .ok()?
            .into_iter()
            .find_map(|(_, service_id)| service_id)?
            .raw_uuid()
    } else {
        manager
            .lookup_username(query)
            .await
            .inspect_err(|error| debug!(%error, "username lookup failed"))
            .ok()??
            .into()
    };

    // A stranger's profile is encrypted under a profile key this account has not
    // been given, so there is no name to fetch: what they were found by is what
    // they are shown as until they answer and their profile key arrives with it.
    let name = manager
        .store()
        .contact_by_id(&presage::libsignal_service::protocol::ServiceId::Aci(uuid.into()))
        .await
        .ok()
        .flatten()
        .map(|contact| contact.name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| query.to_string());

    Some(Contact { uuid, name })
}

fn uuid_from_contact_record(
    record: &presage::libsignal_service::proto::ContactRecord,
) -> Option<Uuid> {
    record
        .aci
        .parse::<Uuid>()
        .ok()
        .or_else(|| Uuid::from_slice(&record.aci_binary).ok())
}

/// Brings names and pictures up to date.
///
/// Both come from the same profile fetch, so they are one pass rather than two
/// walks over the same people. presage's own profile and avatar caches are
/// dropped first, because it has no way to expire them and will otherwise keep
/// answering with whatever it saw the first time.
async fn refresh_profiles(mut manager: RegisteredManager, db: Db, cache: Cache, mut events: Events) {
    if let Err(error) = db.forget_profiles().await {
        warn!(%error, "failed to drop the cached profiles");
    }

    for (uuid, key) in profile_keys(&manager).await {
        let thread = Thread::Contact(ContactId::Aci(uuid));
        let profile = match manager.retrieve_profile_by_uuid(uuid, key).await {
            Ok(profile) => profile,
            Err(error) => {
                debug!(%error, %uuid, "failed to fetch a profile");
                continue;
            }
        };

        if let Some(name) = profile
            .name
            .map(|name| name.to_string())
            .filter(|name| !name.trim().is_empty())
        {
            // Kept before it is shown. This loop is one round trip per person
            // over the whole address book, and whatever it reaches before the
            // window is closed is what the next launch opens with.
            if let Err(error) = db.set_profile_name(uuid, &name).await {
                warn!(%error, %uuid, "failed to store a profile name");
            }
            emit(&mut events, Event::Profile { uuid, name }).await;
        }

        // The avatar's CDN path changes when the picture does, so this is the
        // whole freshness check -- and the reason a refresh is not a download.
        let Some(remote) = profile.avatar.filter(|remote| !remote.is_empty()) else {
            continue;
        };
        if unchanged(&db, &cache, &thread, &remote).await {
            continue;
        }
        match manager.retrieve_profile_avatar_by_uuid(uuid, key).await {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                store_avatar(&cache, &db, &mut events, thread, &remote, &bytes).await;
            }
            Ok(_) => {}
            Err(error) => debug!(%error, %uuid, "failed to fetch a profile avatar"),
        }
    }

    refresh_group_avatars(&mut manager, &db, &cache, &mut events).await;
}

async fn refresh_group_avatars(
    manager: &mut RegisteredManager,
    db: &Db,
    cache: &Cache,
    events: &mut Events,
) {
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
        let thread = Thread::Group(master_key);
        if unchanged(db, cache, &thread, &group.avatar).await {
            continue;
        }
        let context = GroupContextV2 {
            master_key: Some(master_key.to_vec()),
            revision: Some(group.revision),
            ..Default::default()
        };
        match manager.retrieve_group_avatar(context).await {
            Ok(Some(bytes)) if !bytes.is_empty() => {
                store_avatar(cache, db, events, thread, &group.avatar, &bytes).await;
            }
            Ok(_) => {}
            Err(error) => warn!(%error, title = group.title, "failed to fetch group avatar"),
        }
    }
}

/// Whether the cached picture already came from this remote one. Both halves
/// matter: the recorded source can be right while the file has been pruned.
async fn unchanged(db: &Db, cache: &Cache, thread: &Thread, remote: &str) -> bool {
    let recorded = db.avatar_source(thread).await.ok().flatten();
    recorded.as_deref() == Some(remote) && cache.avatar(thread).await.is_some()
}

/// Everyone worth fetching a profile for, with the key that decrypts it.
///
/// A contact record carries a name only if the user typed one on their own
/// phone, so group members and anyone never saved have nothing until this runs.
/// Group membership is where the profile keys for non-contacts come from, and
/// our own comes from the registration -- without it the identity panel shows
/// someone else's idea of us, or nothing.
async fn profile_keys(manager: &RegisteredManager) -> Vec<(Uuid, ProfileKey)> {
    let registration = manager.registration_data();
    let mut wanted = vec![(registration.service_ids.aci, registration.profile_key())];

    match manager.store().contacts().await {
        Ok(contacts) => {
            for contact in contacts.filter_map(Result::ok) {
                if let Ok(key) = <[u8; 32]>::try_from(contact.profile_key.as_slice()) {
                    wanted.push((contact.uuid, ProfileKey::create(key)));
                }
            }
        }
        Err(error) => warn!(%error, "failed to list contacts for profiles"),
    }
    match manager.store().groups().await {
        Ok(groups) => {
            for (_, group) in groups.filter_map(Result::ok) {
                for member in group.members {
                    wanted.push((member.aci.into(), member.profile_key));
                }
            }
        }
        Err(error) => warn!(%error, "failed to list groups for profiles"),
    }

    wanted.sort_by_key(|(uuid, _)| *uuid);
    wanted.dedup_by_key(|(uuid, _)| *uuid);
    wanted
}

async fn store_avatar(
    cache: &Cache,
    db: &Db,
    events: &mut Events,
    thread: Thread,
    remote: &str,
    bytes: &[u8],
) {
    match cache.put_avatar(&thread, bytes).await {
        Ok(path) => {
            if let Err(error) = db.set_avatar_source(&thread, remote).await {
                warn!(%error, "failed to record an avatar source");
            }
            emit(events, Event::Avatar { thread, path }).await;
        }
        Err(error) => warn!(%error, "failed to cache an avatar"),
    }
}

/// Fills the sidebar, from what was kept rather than from what can be rebuilt.
///
/// Two passes, and the order is the point. The stored lines go first: they are
/// one query, they need nothing decoded, and they are what the list looked like
/// when it was last closed. The scan of presage's own rows follows, and only
/// adds -- a line for a thread that has none, and the bare fact of activity for
/// a thread whose newest rows project to nothing, which is a thread that used to
/// drop off the list entirely with its whole history still on disk.
async fn fetch_previews(db: Db, aci: Uuid, mut events: Events) {
    let mut listed = std::collections::HashSet::new();
    match db.previews().await {
        Ok(stored) => {
            for (thread, preview) in stored {
                listed.insert(thread.clone());
                emit(&mut events, Event::Preview { thread, preview }).await;
            }
        }
        Err(error) => warn!(%error, "failed to load the stored previews"),
    }

    let scanned = match db.recent_rows().await {
        Ok(scanned) => scanned,
        Err(error) => {
            warn!(%error, "failed to scan for thread previews");
            return;
        }
    };
    let marks = db.read_marks().await.unwrap_or_default();

    for recent in scanned {
        let thread = recent.thread;
        match data::project(recent.rows).pop() {
            // Written as well as sent: a line the scan had to work for is a line
            // the next launch should be handed.
            Some(mut message) => {
                // The row carries no status: `project` reads presage's own store,
                // which knows nothing about receipts. So the ticks the sidebar
                // draws are resolved here, from what petunia kept -- otherwise a
                // thread whose newest message is ours opens with a line and no
                // mark on it, and the receipt that would have raised one has
                // already been and gone.
                if message.sender() == aci {
                    message.status = Some(stored_status(&db, &thread, message.timestamp()).await);
                }
                let preview = data::index::Preview::of(&message);
                if let Err(error) = db.set_preview(&thread, &preview).await {
                    warn!(%error, "failed to store a thread preview");
                }
                emit(&mut events, Event::Preview { thread: thread.clone(), preview }).await;
            }
            // Nothing to read, but rows all the same. Said so it stays listed.
            None if !listed.contains(&thread) => {
                emit(&mut events, Event::Activity { thread: thread.clone(), at: recent.at }).await;
            }
            None => {}
        }

        // A thread nobody has ever read has no mark, and counting all of its
        // history as unread would put a four-figure badge on every row.
        let Some(&upto) = marks.get(&super::db::read::key(&thread)) else {
            continue;
        };
        match db.unread(&thread, aci, upto).await {
            Ok(0) => {}
            Ok(count) => emit(&mut events, Event::Unread { thread, count }).await,
            Err(error) => warn!(%error, "failed to count unread messages"),
        }
    }
}

/// Publishes everything known about what people are called, before a single
/// profile has been fetched. The crawl that follows makes these current; without
/// them, a crawl that does not finish leaves the rest of the list as uuids.
async fn send_names(db: &Db, events: &mut Events) {
    let known = match db.names().await {
        Ok(known) => known,
        Err(error) => {
            warn!(%error, "failed to load the stored names");
            return;
        }
    };

    for known in known {
        if let Some(name) = known.profile {
            emit(events, Event::Profile { uuid: known.uuid, name }).await;
        }
        if known.nickname.is_some() || known.note.is_some() {
            emit(events, Event::Nickname {
                uuid: known.uuid,
                name: known.nickname,
                note: known.note,
            })
            .await;
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
        .map(|(master_key, group)| Group::from((&master_key, group)))
        .collect();
    emit(events, Event::Contacts { contacts, groups }).await;
    Ok(())
}

/// Publishes the installed packs, writing each sticker's bytes into the media
/// cache on the way past. presage keeps them decrypted in its own store, but
/// nothing can draw bytes -- the renderer needs a path.
async fn send_sticker_packs(manager: &RegisteredManager, cache: &Cache, events: &mut Events) {
    let Some(stored) = installed_packs(manager).await else {
        return;
    };

    let mut packs = Vec::new();
    let mut unreadable = 0;
    for pack in stored {
        match cached_pack(pack, cache).await {
            Some(pack) => packs.push(pack),
            None => unreadable += 1,
        }
    }

    if unreadable > 0 {
        warn!(unreadable, "installed sticker packs have no stickers to draw");
    }
    emit(events, Event::StickerPacks { packs, unreadable }).await;
}

async fn installed_packs(manager: &RegisteredManager) -> Option<Vec<presage::store::StickerPack>> {
    match manager.store().sticker_packs().await {
        Ok(packs) => Some(packs.filter_map(Result::ok).collect()),
        Err(error) => {
            warn!(%error, "failed to list sticker packs");
            None
        }
    }
}

/// Fetches the stickers an installed pack is missing.
///
/// A pack is installed once and its stickers downloaded once, in a burst of a
/// hundred requests -- and whatever fails in that burst is stored with no bytes
/// and never asked for again. The pack stays installed and half of it, or all of
/// it, draws nothing for good. So the packs are checked against what is actually
/// on disk at every launch, and an incomplete one is read again: the manifest is
/// behind the pack key, which is stored beside it, so this needs nothing the
/// account has not already got.
///
/// `preview_sticker_pack` rather than `install_sticker_pack`, because the pack is
/// already installed -- installing it again would tell every other device to do
/// something they did months ago.
async fn repair_sticker_packs(manager: RegisteredManager, cache: Cache, mut events: Events) {
    let Some(stored) = installed_packs(&manager).await else {
        return;
    };

    let mut wanted = Vec::new();
    for pack in stored {
        if missing(&pack, &cache).await > 0 {
            wanted.push((pack.id, pack.key));
        }
    }
    if wanted.is_empty() {
        return;
    }

    info!(packs = wanted.len(), "re-reading incomplete sticker packs");
    let mut store = manager.store().clone();
    let mut repaired = 0;
    for (id, key) in wanted {
        match manager.preview_sticker_pack(&id, &key).await {
            Ok(fetched) => match store.add_sticker_pack(&fetched).await {
                Ok(()) => repaired += 1,
                Err(error) => warn!(%error, "failed to store a re-read sticker pack"),
            },
            Err(error) => warn!(%error, "failed to re-read a sticker pack"),
        }
    }

    if repaired > 0 {
        send_sticker_packs(&manager, &cache, &mut events).await;
    }
}

/// How many of a pack's stickers there is nothing to draw for: no bytes in the
/// manifest and no file in the cache either.
async fn missing(pack: &presage::store::StickerPack, cache: &Cache) -> usize {
    let mut missing = 0;
    for sticker in &pack.manifest.stickers {
        if sticker.bytes.is_none() && cache.sticker(&pack.id, sticker.id).await.is_none() {
            missing += 1;
        }
    }
    missing
}

/// Reads a pack this account does not have and reports what it holds. Nothing is
/// installed by it and no other device is told.
async fn read_pack(
    manager: RegisteredManager,
    cache: Cache,
    mut events: Events,
    pack_id: Vec<u8>,
    key: Vec<u8>,
) {
    let pack = match manager.preview_sticker_pack(&pack_id, &key).await {
        Ok(pack) => cached_pack(pack, &cache)
            .await
            .ok_or_else(|| "none of that pack's stickers could be read".to_owned()),
        Err(error) => {
            warn!(%error, "failed to read a sticker pack");
            Err(format!("could not read that sticker pack: {error}"))
        }
    };
    emit(&mut events, Event::StickerPackPreview { pack_id, pack }).await;
}

/// A pack as the views read it: every sticker's bytes written into the media
/// cache, because nothing can draw bytes. `None` for a pack none of whose
/// stickers could be written, which is a pack there is nothing to show of.
async fn cached_pack(
    pack: presage::store::StickerPack,
    cache: &Cache,
) -> Option<data::stickers::Pack> {
    let mut stickers = Vec::new();
    for sticker in &pack.manifest.stickers {
        let path = match cache.sticker(&pack.id, sticker.id).await {
            Some(path) => Some(path),
            None => match &sticker.bytes {
                Some(bytes) => cache
                    .put_sticker(&pack.id, sticker.id, bytes)
                    .await
                    .inspect_err(|error| warn!(%error, "failed to cache a sticker"))
                    .ok(),
                None => None,
            },
        };
        if let Some(path) = path {
            stickers.push(data::stickers::Sticker {
                id: sticker.id,
                emoji: sticker.emoji.clone(),
                path,
            });
        }
    }

    if stickers.is_empty() {
        return None;
    }
    Some(data::stickers::Pack {
        id: pack.id,
        key: pack.key,
        title: pack.manifest.title,
        author: pack.manifest.author,
        stickers,
    })
}
