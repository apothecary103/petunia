# Where the rewrite stands

Petunia reads and writes a real conversation. The sidebar, the message list, the
composer, the media viewer, playback and the details panel are all live against
a linked account.

411 tests pass and `cargo clippy --workspace --all-targets` is clean. Inside
`nix develop` (or with direnv, just `cd`): `cargo test`, and
`cargo build && ./target/debug/petunia` to run it.

## What works

- **Sending.** Text, replies, edits, deletes, reactions, attachments and
  stickers. Formatting is typed as markers (`**bold**`, `*italic*`, `~strike~`,
  `` `mono` ``, `||spoiler||`) and turned into Signal's `BodyRange` offsets by
  one pure function; the toolbar buttons wrap the selection in those markers.
  Enter sends, shift+enter breaks the line, escape drops whatever the composer
  is carrying, up on an empty field edits your last message.
- **Receiving.** Runs with day separators and an unread marker, rich text,
  quotes, reactions, stickers, link previews, captions, and system lines. Reading
  backwards needs no button: reaching the top of the loaded page is what asks for
  the one behind it.
- **Drafts.** Per conversation, and everything the composer is carrying rather
  than only the words -- a reply banner and a picked-out file were meant for the
  conversation they were chosen in. The words alone survive a restart, since an
  attachment is a path and a path is not a promise.
- **Media.** Images resampled to the display's real pixel count, video with a
  generated poster frame, voice notes with Signal's own waveform, file chips
  with open and save. Anything picture-shaped opens in a viewer with zoom, pan, a
  rail of the rest of the thread, copy, save-as and hand-off -- a panel over the
  conversation rather than edge to edge, so what it came from is still there.
- **Playback.** Voice notes and audio through rodio; video through AVFoundation
  into gpui's surface element, with play, scrub and a clock.
- **Groups.** Members with roles and the labels Signal lets people pick for
  themselves, descriptions, invite and request counts, the disappearing timer.
- **Receipts.** A tick beside the text rather than a line of its own, read state
  written down, and reading on another device clears the badge here.
- **Markdown**, both ways: what you type is parsed into `BodyRange`s on send, and
  what arrives with no ranges is parsed for display. Fenced blocks get a box and
  tree-sitter highlighting in the theme's own syntax palette.
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
  Keybinding presets: standard, emacs, vim.
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
- **Emoji picker.** The sticker picker's shape carries over.
- **Mention autocomplete** in the composer, and `:shortcode:` completion. The
  mention *rendering* side is done.
- **Notifications.** `[notifications]` is settable and nothing reads it, which
  the settings window says out loud rather than pretending otherwise.
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
- **Group mutation** — creating, renaming, adding and removing — is not possible
  through presage: `GroupOperations` builds the actions but there is no PATCH
  endpoint wrapper.
- **Sending a sticker re-uploads its bytes.** presage's stored pack keeps the
  decrypted image and drops the `AttachmentPointer` it arrived under, so there
  is nothing to forward.
