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
  `themes.example.toml`. A theme carries a full semantic colour token set, a
  syntax palette for code blocks, and typography, and is hot-reloaded. Petunia's
  own `dark` and `light` are neutral greys, no hue, spending their only bright
  value on the send button — not Catppuccin, and not any other borrowed scheme.
  Zed's eleven ship alongside them, converted by `script/zed-themes.py` and
  compiled in.

## Layout

Follow the reference design, not the layout the old iced client used. The iced
version is gone and its arrangement is not a precedent for anything.

- **Sidebar.** Runs the full height of the window, with the traffic lights
  floating over its own top padding. Small quiet section headers and two-line
  entries carrying real metadata. Identity sits at the very bottom. Its right
  edge is draggable, and dragged in far enough it collapses to a **rail** of
  avatars — nothing on it is a line of text, since text is the one thing that
  width has no room for; the badges (unread, typing) go in the corners of the
  picture instead. The width is a preference in `config.toml`, written on release
  and set nowhere else — the settings window offers no stepper for it, because two
  controls for one number is a promise they cannot disagree. Whether it is a rail
  is session state.
- **Header.** Belongs to the conversation column, not the whole window — a
  full-width strip leaves a dead band above the sidebar. No rule beneath it;
  the columns are what separate things.
- **Composer.** A rounded card floating over the conversation with its controls
  *inside* it, right-aligned, and a thin context strip beneath.
- **Chrome.** Generous vertical rhythm, hairline borders, rounded chips, muted
  secondary text.
- **Attachments.** A text file is drawn as its own first lines, highlighted, the
  way it would read had it been pasted rather than attached — `ui/message/text.rs`
  decides what counts as text and reads the head of it once. Everything else is
  the chip in `ui/message/media.rs`.
- **Messages.** Three layouts, chosen in settings and built from shared parts in
  `ui/message/run.rs`: **standard**, Discord-style runs with an avatar gutter, one
  header per run and a hanging indent; **compact**, one line per message with the
  clock and the name in fixed right-aligned columns, as every IRC client draws it;
  and **bubbles**, Signal's own, yours on the right. The one place that departs
  from the reference, which is not a chat app. Independent of `density`, which is
  how much room a message is given rather than what shape it is.
- **The reading column** is capped and pinned to the left of its column, not
  centred. Centring moves every message sideways when the window is resized or a
  panel opens.

Anything with a visual opinion goes in `ui/kit.rs`, so the look is one
decision rather than a per-view accident.

## Source layout

A cargo workspace, so the dependency direction is enforced by the compiler
rather than by good intentions. Each crate may only reach the ones below it.

- `crates/data` — `petunia-data`, the model the views read: threads, messages,
  history, the sidebar index, attachments. Depends on nothing of ours, and
  **nothing of gpui's** — it does not even have it as a dependency, so an
  accidental import will not compile.
- `crates/config` — `petunia-config`: `config.toml`, themes, keybindings.
  Depends on `data` for the one preference that names a model type.
- `crates/media` — `petunia-media`: `audio` and `video`. Each owns its output
  device on a thread the window does not touch, and neither has gpui either.
- `crates/signal` — `petunia-signal`, the engine: the worker thread, the
  command/event protocol, presage's store, petunia's own sqlite tables, and the
  media cache. `spawn` starts it; the caller never sees the thread.
- `crates/petunia` — the binary. `ui/` and everything gpui, `store.rs` (the one
  entity views observe, and the only way they talk back to the worker),
  `bridge.rs` (the worker's events into the store), `theme.rs`, `actions.rs`,
  `session.rs`.

Shared dependency versions live in the root `[workspace.dependencies]`; a crate
manifest says `foo.workspace = true` and nothing else.

Every control drawn on a message reports through one closure
(`ui::message::act::Act`), which the conversation answers in a single match.
Adding a control is a variant and an arm, not another callback threaded through
four call sites.

## Building

`nix develop` (or `direnv allow`, via `.envrc`) is the shell this builds in, and
`cargo build --workspace` inside it works from cold.

Two things have to be on `PATH` and only one of them is packaged. `protoc`,
because two dependencies (presage's protocol crates and `spqr`) generate code
from `.proto` files at build time — a `target/` full of cached artifacts hides
this until something invalidates one of them, and the failure then names `spqr`
rather than the missing tool. And Xcode's `metal`, for gpui's shaders, which is
not in nixpkgs and lives behind a cryptex mount whose path changes with every
toolchain update. `flake.nix` asks `xcrun` where it is, with `DEVELOPER_DIR`
unset for the question only: nixpkgs points that at the SDK from the store,
which carries no toolchain, so `xcrun -f metal` under it answers "not found".

Anything that *replaces* the environment rather than adding to it — `nix-shell
-p protobuf`, say — satisfies the first and breaks the second.

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
- **`dirs::config_dir()` is not `~/.config` on macOS.** It is Application
  Support, which is where an application keeps its own state, not where a person
  keeps a file they edit. `config::dir()` prefers `~/.config/petunia` there, as
  Zed does, and falls back to the old location when the file is already in it.
- **There is no tokio runtime on the UI side.** `tokio::time::timeout` and
  friends panic outright there; use `cx.background_executor().timer`. The Signal
  worker has its own runtime and is the only place tokio's timers are safe.
- **gpui's blur is a tint, not a blur.** `WindowBackgroundAppearance::Blurred`
  puts an `NSVisualEffectView` across the *whole* window using the `Selection`
  material, through which the desktop is simply there, in focus. `ui::vibrancy`
  asks for `Transparent` instead and puts its own view behind the conversation
  list with the `HUDWindow` material, the thickest frost AppKit offers, under an
  appearance taken from the theme rather than the system. The semantic `Sidebar`
  material is too thin to read through the list's own fill, which stays mostly
  opaque for the list to be legible — at alpha 0.97 the blur was invisible
  altogether, which reads as "the blur is broken".
- **`gpui_component::Root` paints the theme background** across the window. It
  covers anything behind it, vibrancy included; it implements `Styled`, so
  `main` clears it when the sidebar is translucent.
- **gpui's `surface` asserts its pixel format.** It builds two Metal textures
  from a buffer's luma and chroma planes and requires bi-planar YCbCr; anything
  else aborts the process from inside the renderer with two integers for a
  message. `video.rs` checks before handing one over.
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
- **A scrolling `div` builds every child every frame.** A scroll notifies the
  view that owns the handle, so the whole element tree is rebuilt and re-laid
  out — for a thread, that is every message in the history per frame of every
  flick. The conversation uses gpui's `list` with `ListAlignment::Bottom`, which
  builds only what is on screen plus `OVERDRAW`. Its scroll position is an item
  index and an offset into it, so rows spliced in at the *front* carry the reader
  with them and loading older messages needs no correction; row zero is always
  present, even when it draws nothing, because a row that came and went would
  shift every index behind it.
- **A visible row is re-rendered every frame**, so anything derived in one has to
  be cheap or cached. Two things here were not: `Theme::highlights` is a round
  trip through Zed's theme JSON (~260µs), now built once per theme install and
  read from the `Palette` global; and tree-sitter reparses a code block from
  scratch (~600µs), now memoized in `ui::message::content` on the code, the
  language and the theme.
- **Sender colours are generated, not picked from a list.** A palette of eight
  runs out at the ninth person in a group, and eight *muted* colours -- which is
  what a neutral theme wants -- are eight shades of one idea. `Theme::accent_for`
  hashes the uuid (FNV-1a; the byte sum it used before only ever read three bits
  of it) and snaps it to one of twenty-four hues and one of four tones, so two
  people are either the same colour or a clear fifteen degrees apart. A theme file
  that lists `accents` still gets exactly those; petunia's own two list none.
- **Focusing a sheet steals focus from the field inside it.** `window.focus` on an
  overlay's own handle takes the focus off the text input the overlay just focused,
  and the result is a search box that looks active and cannot be typed into -- the
  bug cmd+f had. An overlay with an input focuses the *input* and nothing else;
  actions still reach the overlay, because dispatch walks up the element tree from
  whatever holds the focus.
- **`FontWeight::MEDIUM` draws as Regular on macOS.** gpui hands font-kit a CSS
  weight and font-kit matches per CSS Fonts 3. The system family's faces report
  CoreText's own weights, and the conversion puts Medium at 530 rather than 500 —
  so a request for 500 finds no exact match, hits the rule that checks 400 first,
  and rasterises `.SFNS-Regular`. Semibold (600) and Bold (700) land exactly.
  `kit::EMPHASIS` and `kit::STRONG` are the two weights above regular that macOS
  actually draws; nothing should name a weight directly.
- **A div only hears a mouse move while it is under the pointer.** Which is the
  one thing a drag stops being — so a resize written as `on_mouse_move` on an
  ancestor updates on release and never during, the symptom the sidebar's divider
  had. The pointer has to be followed at the window level
  (`Window::on_mouse_event`), reachable from a `canvas`'s paint closure without
  writing a whole element; `ui::workspace::follow` is the one.
- **A scroll wheel goes through an overlay on purpose.** `Hitbox::is_hovered` is
  occluded by whatever is in front, but `should_handle_scroll` is not, so that an
  overlay does not stop the page under it scrolling. A modal wants the opposite:
  every full-window sheet calls `occlude`, and `kit::scrim` does it for the ones
  built on that.
- **A highlight has no padding.** `HighlightStyle` is a colour over a range of
  text, so inline code cannot be given a padded rounded box the way markdown draws
  one. `ui::message::content` pads it with thin spaces *inside* the washed run
  instead, and the wash is `active` rather than `sunken` — sunken is a shade below
  the background, which on a dark theme reads as nothing at all.

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
