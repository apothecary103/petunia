# Petunia

Petunia is a native Signal client, named after the flower.

- **Signal library:** [`presage`](https://github.com/whisperfish/presage) — follow
  its examples for how to link, send, and receive.
- **GUI framework:** [`gpui`](https://gpui.rs), taken from the
  [Zed](https://github.com/zed-industries/zed) repository, with
  [`gpui-component`](https://github.com/longbridge/gpui-component) for the
  window root, icons and text input.
- **Look and feel:** modeled on Zed — a collapsible left sidebar, one focused
  conversation, and an optional right details panel. No tabs, and no movable
  pane grid.
- **Themes:** TOML files in `~/.config/petunia/themes/`, documented by
  `themes.example.toml`. A theme carries a full semantic colour token set plus
  typography, and is hot-reloaded. The built-ins are `dark` and `light` —
  neutral greys, no hue, spending their only bright value on the send button.
  Not Catppuccin, and not any other borrowed scheme.

## Layout

Follow the reference design, not the layout the old iced client used. The iced
version is gone and its arrangement is not a precedent for anything.

- **Sidebar.** Runs the full height of the window, with the traffic lights
  floating over its own top padding. Small quiet section headers and two-line
  entries carrying real metadata. Identity sits at the very bottom.
- **Header.** Belongs to the conversation column, not the whole window — a
  full-width strip leaves a dead band above the sidebar. No rule beneath it;
  the columns are what separate things.
- **Composer.** A rounded card floating over the conversation with its controls
  *inside* it, right-aligned, and a thin context strip beneath.
- **Chrome.** Generous vertical rhythm, hairline borders, rounded chips, muted
  secondary text.
- **Messages.** Discord-style runs: an avatar gutter, one header per run,
  hanging indent. The one place that departs from the reference, which is not a
  chat app.

Anything with a visual opinion goes in `src/ui/kit.rs`, so the look is one
decision rather than a per-view accident.

## Source layout

- `src/signal/` — the Signal engine: the worker thread, the command/event
  protocol, presage's store, petunia's own sqlite tables, and the media cache.
  Framework-agnostic apart from `bridge.rs`.
- `src/data/` — the model the views read: threads, messages, history, the
  sidebar index. Framework-agnostic; **no gpui imports belong here**.
- `src/config/` — `config.toml`, themes, and keybindings.
- `src/store.rs` — the one entity views observe, and the only way they talk back
  to the worker.
- `src/audio.rs`, `src/video.rs` — playback. Each owns its output device on a
  thread the UI does not touch, and neither imports gpui.
- `src/ui/` — everything gpui.

Every control drawn on a message reports through one closure
(`ui::message::act::Act`), which the conversation answers in a single match.
Adding a control is a variant and an arm, not another callback threaded through
four call sites.

## Building

`protoc` must be on `PATH`; two dependencies (presage's protocol crates and
`spqr`) generate code from `.proto` files at build time. A `target/` full of
cached artifacts hides this until something invalidates one of them, and the
failure then names `spqr` rather than the missing tool.

Xcode's `metal` must be on `PATH` too, for gpui's shaders. Anything that
*replaces* the environment rather than adding to it — `nix-shell -p protobuf`,
say — will satisfy the first and break the second.

## Traps

Every one of these has already cost a debugging session.

- **Two copies of gpui.** `gpui-component` declares a *bare* git dependency on
  the Zed repo. Cargo treats `git = url` and `git = url, rev = ...` as different
  sources, so pinning a revision silently builds a second, type-incompatible
  gpui. The dependency is deliberately unpinned; `Cargo.lock` does the pinning.
  Check with `grep -c '^name = "gpui"$' Cargo.lock` — it must be `1`.
- **gpui-ce does not work here.** The community fork is missing what
  gpui-component uses (`flex_grow_1`, `container_query`). It was tried and
  abandoned.
- **Icons need the asset source.** `application().with_assets(...)` must be
  passed `gpui_component_assets::Assets`, or every icon silently draws nothing.
- **Actions dispatch along the focus path.** If nothing has claimed focus, no
  keybinding fires anywhere. The workspace holds a `FocusHandle` and takes focus
  at launch.
- **Timestamps are milliseconds**, but `group_within` is configured in seconds.
  Convert at the boundary.
- **Commands sent before the worker reports in** used to be dropped. The window
  is clickable a second or two before `Event::Ready`; `Store` queues and flushes.
- **`max_w`/`max_h` on an image is not enough.** An image's natural size is its
  pixel size, so the fitted size is computed from the pointer's dimensions.
- **`clear_key_bindings` clears everyone's.** `actions::bind` wipes the whole
  keymap, including what `gpui_component::init` installs for its own text input.
  The library therefore has to be initialised *after* it, and the theme after
  that, because `init` seeds its own palette. `main::install` is the one place
  that order lives.
- **gpui minifies an image in one bilinear tap.** It uploads at the source's
  pixel size and samples with no mipmaps, so a 640px avatar in a 34pt circle
  aliases hard — this is the "pixelated on a retina display" symptom. Never call
  `img()` directly; `ui::image::picture`/`cropped` resample to the device pixels
  the element actually occupies.
- **A layout closure does not know how wide it ended up.** Anything that turns a
  click into a fraction of an element — the waveform, the video scrubber — needs
  a `canvas` behind it to record the bounds it was laid out at.
- **presage reads an attachment whole** before it can verify the digest, so
  there is no byte count to report while one downloads. `Blob::Downloading`
  carries no fraction and the bar slides rather than fills.
- **Two crates wrap `CVPixelBuffer`.** gpui's `surface` takes `core-video`'s;
  AVFoundation's bindings produce `objc2-core-video`'s. Same pointer, so the
  conversion is a cast under a create rule (`video.rs`).

## Coding rules

- **KISS.** The simplest thing that works. No abstraction for features that don't
  exist yet.
- **SOLID.** Each module has one responsibility. Depend on interfaces at
  boundaries. Extend by adding, not by editing shared code.
- **Clean code.** Small functions, clear names, no dead code, no speculative
  generality.
- **No comments** unless absolutely necessary — code should explain itself.
  Comment only what stays genuinely surprising (protocol quirks, invariants,
  `unsafe`).
- **No false affordances.** A control that looks like it does something must do
  it, or not be drawn.

See `TODO.md` for what is left.
