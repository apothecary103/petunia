# Where the rewrite stands

Petunia reads and writes a real conversation. The sidebar, the message list, the
composer, the media viewer, playback and the details panel are all live against
a linked account.

385 tests pass and `cargo clippy --workspace --all-targets` is clean. Inside
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
  quotes, reactions, stickers, link previews, captions, and system lines.
- **Media.** Images resampled to the display's real pixel count, video with a
  generated poster frame, voice notes with Signal's own waveform, file chips
  with open and save. Anything picture-shaped opens full size in a viewer with
  zoom, pan, a rail of the rest of the thread, copy, save-as and hand-off.
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
  screen, cmd+k to jump to a conversation.
- **The list.** Pinning, archiving, muting and flat folders, from right-click
  menus; menus on messages and people too. Deleting a conversation asks first and
  clears this device's copy -- Signal's own "delete for me" goes through the
  Storage Service, which presage does not expose, so it cannot reach the phone.
- **Settings** (cmd+,) over every preference, writing `config.toml` so the file
  stays authoritative. Keybinding presets: standard, emacs, vim.
- **Themes.** Petunia's two, and all eleven of Zed's, compiled in.
- **Chrome.** Keyboard sheet (cmd+/), conversation cycling, error notices, hot
  reload, and a translucent sidebar over the desktop on macOS.

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
- **Per-thread drafts** — body and ranges only, never attachment paths.
- **Draggable panel edges.** `session.json` carries a width for the sidebar and
  the details panel, and nothing but a hand edit can change either. A narrow
  window already lends their width back to the conversation; being able to set it
  is the other half.
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
