# Where the rewrite stands

The client is on gpui and reads real conversations against a linked account:
sidebar, message list with runs and day separators, inline images, stickers,
attachment chips, quotes, reactions, rich text, receipts, a details panel with
profiles and shared media, the cmd+k switcher, themes and hot reload.

231 tests pass. `cargo test`, and `cargo build && ./target/debug/petunia` to run
it against the real account.

Nothing sends yet.

---

## Finish phase 3 first

These are the gaps in what is already on screen. The first two matter most.

- **Hover action bar** — react, reply, edit, delete, copy on a hovered run.
  Deliberately not built yet: every button needs a command path from phase 4,
  and drawing five controls that do nothing would be five false affordances.
  Build it *with* phase 4, not before.
- **`ListState`** — the message list is still a plain `overflow_y_scroll` div
  with `justify_end`. Paging older messages can therefore jump the viewport.
  Move to `gpui::list()` with `ListState`, splicing at index 0 for prepends, and
  test by fast-scrolling to the top of a long thread. This is the one risk from
  the original plan that is still unretired.
- **Spoiler reveal** — spoilers render as blocks but clicking does nothing.
  Needs per-span hit-testing (`InteractiveText`), and reveal state keyed by
  message timestamp and segment start, held on `Conversation`.
- **Link opening** — URLs are styled but inert. Same hit-testing problem; hand
  off to the `open` crate as the iced build did.
- **Details panel** — group member list, disappearing-message timer, and the
  contact's phone number are not shown.

## Phase 4 — sending

`src/signal/{command,outgoing,worker}.rs` already implement all of this; none
of it is wired to a view.

- Composer on `gpui_component::input::InputState` (multiline). Enter sends,
  Shift+Enter newlines, Escape cancels context, Up-when-empty edits your last
  message. The current composer is a static card — it does not accept text.
- The `Aa` formatting toolbar toggles but its buttons do nothing; wire them to
  wrap the selection in the matching `Range`.
- Reply and edit banners, attachment strip, `rfd` file picker (cmd+u),
  drag-and-drop.
- Optimistic echo — `History::apply_locally`/`echo` already exist.
- Typing indicators out (10s re-send throttle, stop on send); read receipts on
  focus. `State::typing`/`expire_typing` exist and need a 1s tick to drive them.
- Toast notices; `notice.rs`'s state machine is in git history at `73cc303^`.

**Note:** `rfd` is configured with `default-features = false, features =
["xdg-portal"]`, which is Linux-only. The file picker will not work on macOS
until that is fixed.

## Phase 5 — overlays

- Help overlay (cmd+/) generated from `config.keys.listing()`.
- In-app media viewer with zoom, pan and next/previous, replacing the hand-off
  to `open`.

## Phase 6 — new features

- **Desktop notifications.** `[notifications]` is parsed and has zero consumers.
  Add `notify-rust`; respect `enabled`, `show_content`, `show_sender`, `groups`.
  Suppress when the window is focused and the thread is active.
- **Pinned / archived / muted.** `Index::Flags` and `Section::{Pinned,Requests,
  Archived}` exist with tests and no production caller. Migration 003:
  `petunia_thread_flags(thread PK, pinned, archived, muted_until, folder)`.
  The sidebar already renders the sections; they are simply always empty.
- **Folders.** The `folder` column above; flat, one per conversation.
  `Section` gains a `Folder(String)` variant.
- **Message search.** Migration 004, FTS5 over presage's `thread_messages`.
  Needs scroll-to-message, which depends on `ListState` landing first.
- **Sticker sending.** Probe what presage exposes before scoping; may be limited
  to already-installed packs.

## Phase 7 — polish

- `cargo clippy --all-targets` has not been run since the rewrite began.
- Empty states, focus rings.
- `src/signal/cache.rs` names cached files by sniffed extension because *iced*
  picked decoders by extension. Verify whether gpui's image loader cares; if it
  sniffs bytes, that cleverness is dead and the comments are wrong.
- Squash-friendly history, PR to `main` from `gpui-rewrite`.
