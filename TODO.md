# Where the rewrite stands

Petunia reads and writes a real conversation. The sidebar, the message list, the
composer, the media viewer, playback and the details panel are all live against
a linked account.

464 tests pass and `cargo clippy --workspace --all-targets` is clean. Inside
`nix develop` (or with direnv, just `cd`): `cargo test`, and
`cargo build && ./target/debug/petunia` to run it.

## What works

- **Sending.** Text, replies, edits, deletes, reactions, attachments and
  stickers. Formatting is typed as markers (`**bold**`, `*italic*`, `~strike~`,
  `` `mono` ``, `||spoiler||`) and turned into Signal's `BodyRange` offsets by
  one pure function; the toolbar buttons wrap the selection in those markers, and
  in the two Signal has no style for -- a fence and a pair of dollars.
  Enter sends, shift+enter breaks the line, escape drops whatever the composer
  is carrying, up on an empty field edits your last message.
- **Receiving.** Runs with day separators and an unread marker, rich text,
  quotes, reactions, stickers, link previews, captions, and system lines. Reading
  backwards needs no button: reaching the top of the loaded page is what asks for
  the one behind it.
- **Starting a conversation.** cmd+n, the `+` above the conversation list, or
  Message ▸ New Conversation. Contacts by name, or anybody at all by username or
  phone number, which is a lookup against Signal rather than a search here. The
  same picker makes a group: pick everybody, then name it.
- **Drafts.** Per conversation, and everything the composer is carrying rather
  than only the words -- a reply banner and a picked-out file were meant for the
  conversation they were chosen in. The words alone survive a restart, since an
  attachment is a path and a path is not a promise.
- **Media.** Images resampled to the display's real pixel count, video with a
  poster generated at thumbnail size a tenth of the way in, voice notes with
  Signal's own waveform, file chips with open and save. A sticker opens the pack
  it came from -- read from the server rather than installed when it is one this
  account does not have, so its name, its author and the whole of it are on the
  sheet before the decision to add it. Anything picture-shaped opens in a viewer with zoom, pan, a
  rail of the rest of the thread, copy, save-as and hand-off -- a panel over the
  conversation rather than edge to edge, so what it came from is still there.
- **Playback.** Voice notes and audio through rodio; video through AVFoundation
  into gpui's surface element, with play, scrub and a clock.
- **Groups.** Members with roles and the labels Signal lets people pick for
  themselves, descriptions, invite and request counts, the disappearing timer.
- **Notifications.** Real desktop banners, honouring every part of
  `[notifications]`. Nothing for the conversation on screen in a focused window,
  and nothing from a muted one.
- **Blocking.** Through the `blocked` flag on the Storage Service contact record,
  which is where Signal keeps its block list, so it reaches every linked device.
  Anything a blocked person sends is dropped rather than stored and hidden. Both
  it and the nickname are offered by right-clicking the conversation in the list,
  not only from the details panel.
- **Deleting a message**, both ways Signal means it: withdrawn from everybody
  within the day it was sent, or dropped from this account and synced to its other
  devices. The menu asks rather than assuming.
- **Nicknames.** A name you choose for somebody, and a note beside it, both
  through Storage Service so they follow you to your phone.
- **One picker with two tabs,** stickers and emoji, behind the single control in
  the composer. The emoji half is Unicode's own list through the `emojis` crate:
  nine groups drawn as icons, searchable by CLDR name and by shortcode, so a new
  Unicode release is a version bump rather than an edit. The sticker half keeps
  favourites, put there by right-clicking one.
- **Receipts.** A tick beside the text rather than a line of its own, read state
  written down, and reading on another device clears the badge here.
- **Markdown**, both ways: what you type is parsed into `BodyRange`s on send, and
  what arrives with no ranges is parsed for display. Inline code is a chip with
  real padding rather than a wash the shape of the letters; fenced blocks get a box,
  tree-sitter highlighting in the theme's own syntax palette, and a bar carrying the
  language and a copy button.
- **Maths.** `$…$` and `$$…$$` drawn as the symbols they spell, in a serif with
  the variables in italics and everything else upright, and with the source going
  out as typed since nothing else renders it.
  Unicode, not a typesetting engine: symbols are exact, scripts are raised where
  there is a glyph for them, and a fraction is a slash. Display maths gets a line
  of its own, set a third larger than the words around it.
- **Finding things.** cmd+f over every conversation, cmd+shift+f over the one on
  screen, cmd+k to jump to a conversation. A result shows where it matched, and
  choosing one opens the conversation, pages back until the message is loaded and
  lights it.
- **Forwarding**, to a conversation picked the way the switcher picks one. A new
  message rather than a marked forward: the flag lives in a part of the protocol
  presage does not expose.
- **Message details**, from the right-click menu: the fields worth naming and the
  whole structure underneath, copyable.
- **The list.** Pinning, archiving, muting and flat folders, from right-click
  menus; menus on messages and people too. Deleting a conversation asks first and
  clears this device's copy -- Signal's own "delete for me" goes through the
  Storage Service, which presage does not expose, so it cannot reach the phone.
- **Settings** (cmd+,) over every preference, six pages down a rail rather than
  one column of everything, in cards with the theme behind a select rather than
  thirteen chips, writing `config.toml` so the file stays authoritative.
  One set of keybindings rather than three presets, with `ctrl+p` and `ctrl+n`
  and the arrow keys working everywhere.
- **Themes.** Petunia's two, and all eleven of Zed's, compiled in.
- **Chrome.** The macOS menu bar, keyboard sheet (cmd+/), conversation cycling,
  error notices, hot reload, and a translucent sidebar over the desktop.

## What is left

Roughly in the order it would be worth doing.

- **The sidebar is still a plain `overflow_y_scroll` div,** so it rebuilds every
  row on every frame of every flick — the trap the message list was converted off
  in favour of `gpui::list()`. It is bounded by the conversation count rather than
  by the history, which is why it has not bitten yet, but a row still builds a
  preview summary and a relative time per frame. Flattening the sections into
  rows the way `ui::message::group` does would let it use `list()` too.
- **Spoiler reveal and link hit-testing.** Spoilers render as blocks and URLs
  are styled, but only whole-message clicks are routed. Both need per-span
  hit-testing (`InteractiveText`), with reveal state keyed by message timestamp
  and segment start.
- **Mention autocomplete** in the composer, and `:shortcode:` completion in the
  field itself -- the picker already searches by shortcode, so the table behind it
  is there. The mention *rendering* side is done.
- **A draggable details edge.** The conversation list has one, and dragging it in
  collapses it to a rail of avatars; the details panel's width is still session
  state nothing but a hand edit can change.
- **Jump to a quoted message.** The quote block is deliberately not clickable:
  a target outside the loaded page needs a `Command::LoadAround` that does not
  exist, and a control that silently does nothing is worse than none.
- **Disappearing messages.** The timer is read and shown; nothing expires
  anything. presage negotiates the timer but never deletes, and never stores
  `expirationStartTimestamp`, so the reaper is ours.
- **Squash-friendly history, and a PR to `main` from `gpui-rewrite`.**

## Known limits

- **Pins, archives and folders are local.** Signal keeps the first two in its
  Storage Service, which libsignal-service exposes read-only and presage neither
  uses nor exposes; folders it has no concept of. They survive a restart; they do
  not follow you to your phone.

- **Download progress is indeterminate.** presage hands back a whole attachment
  and verifies the digest before it returns, so there is no byte count to show.
  A determinate bar would need `PushService`, which presage keeps `pub(crate)`.
- **Video is macOS-only,** because gpui's surface element is. Elsewhere the
  viewer says so and offers the system player.
- **Nothing from before this device was linked.** A linked device starts with an
  empty history: Signal's servers keep no archive to replay, and the message
  queue only holds what arrived after linking. The official clients fill this in
  with link-and-sync, where the phone uploads an encrypted Backup v2 archive for
  the new device to import — `ephemeral_backup_key` in `provisioning` is the one
  end of that which exists here, and nothing reads it. Importing the archive
  means implementing the whole backup format, which is its own project. Until
  then, history here starts the day you linked.
- **Group mutation past creation** — renaming, adding and removing — still needs
  the PATCH endpoint wrapper nobody has written. Creating one works:
  `Manager::create_group` generates the master key, fetches an expiring profile
  key credential per member, and puts the encrypted group. Anybody whose profile
  key this account does not hold is invited rather than added, because vouching
  for a member means presenting a credential over their profile key.
  **Unverified against a live account:** the reserve/confirm/put paths it goes
  through are all marked so in libsignal-service.
- **Sending a sticker re-uploads its bytes.** presage's stored pack keeps the
  decrypted image and drops the `AttachmentPointer` it arrived under, so there
  is nothing to forward.
- **The sidebar is a scrolling `div`,** so every conversation in it is rebuilt on
  every frame of every flick — the trap the conversation column uses gpui's `list`
  to avoid. Each row is cheap on purpose (`Index::Preview` holds the line, not the
  message), which is what makes it bearable rather than fixed. Virtualising it is
  not a like-for-like swap: `list` keeps its scroll position as an item index, and
  a reorder here is an arbitrary permutation rather than a splice, so a message
  arriving would jump the list unless the position is mapped through the new order
  first. `uniform_list` does not fit either, the section headers not being row
  height.
