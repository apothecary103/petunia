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
  floating over its own top padding — a band that exists *for* them, so it
  collapses where there are none: off macOS, and in fullscreen, where macOS takes
  them away. Small quiet section headers and two-line entries carrying real
  metadata. Identity sits at the very bottom: the display name and the connection,
  and nothing that changes under the pointer — hovering it used to swap the name
  for the username, which meant the one line that says who you are was the one
  line that would not hold still, for a string that has a settings pane of its
  own. Its right
  edge is draggable, and dragged in far enough it collapses to a **rail** of
  avatars — nothing on it is a line of text, since text is the one thing that
  width has no room for; the badges (unread, typing) go in the corners of the
  picture instead. The band the traffic lights float in is the one thing the rail
  cannot share with them — at eighty pixels it is barely wider than they are, and
  the compose button at its right edge sat underneath the one that maximises the
  window — so on the rail the band is left to them and `+` takes the row below,
  centred like everything else on it. The width is a preference in `config.toml`, written on release
  and set nowhere else — the settings window offers no stepper for it, because two
  controls for one number is a promise they cannot disagree. Whether it is a rail
  is session state.
- **Header.** Belongs to the conversation column, not the whole window — a
  full-width strip leaves a dead band above the sidebar. No rule beneath it;
  the columns are what separate things.
- **Composer.** A rounded card floating over the conversation with its controls
  *inside* it, right-aligned, and a thin context strip beneath. The `+` is a menu
  rather than a button, because attaching a file and starting a poll are the same
  kind of act and a permanent button each said they were equally often wanted.
- **Pickers.** One control opens both, and what it opens is a panel with two tabs
  (`ui/composer/picker.rs`). A sticker and an emoji are the same gesture — reach
  for a picture, put it in the message — and two buttons beside each other were
  two places to learn for one intention. Stickers are the default tab, because
  they are the half with no other way in: an emoji can be typed. The emoji rail
  names its nine Unicode groups with *icons*, petunia's own, not with one emoji
  each — a rail of nine emoji over a grid of nothing but emoji has nothing about
  it that says which row is the label. Right-clicking a sticker keeps it to hand;
  the favourites tab is always drawn, empty or not, since a tab that appears only
  once something is in it is a tab nobody finds out how to fill. What is kept is
  a reference into the installed packs (`crates/petunia/src/favourites.rs`, its
  own file beside `session.json`, because the session is written once on quit and
  a favourite is kept the moment it is picked).
- **Chrome.** Generous vertical rhythm, hairline borders, rounded chips, muted
  secondary text.
- **Attachments.** A text file is drawn as its own first lines, highlighted, the
  way it would read had it been pasted rather than attached — `ui/message/text.rs`
  decides what counts as text and reads the head of it once. Everything else is
  the chip in `ui/message/media.rs`.
- **Messages.** Three layouts, chosen under *Appearance* — what shape a message is
  drawn in is what it looks like, not what it says — and built from shared parts in
  `ui/message/run.rs`: **standard**, Discord-style runs with an avatar gutter, one
  header per run and a hanging indent; **compact**, one line per message with the
  clock and the name in fixed right-aligned columns, as every IRC client draws it;
  and **bubbles**, Signal's own, yours on the right. The one place that departs
  from the reference, which is not a chat app. Independent of `density`, which is
  how much room a message is given rather than what shape it is. A bubble is for
  words: a sticker and a message that is nothing but pictures are drawn without
  one, because a rounded box around a photograph is a second frame around a frame
  and Signal draws neither in one.
- **Stickers.** Clicking one opens `ui/sticker.rs` — the picture at a size worth
  looking at, what the pack is called, and the rest of it. Not an install: a click
  that silently added a pack to your account, with a tooltip for a warning, was one
  gesture doing something nobody asked it to. Adding is a button on the sheet, and
  it is the only one drawn, because a pack already here has nothing left to offer
  and a control reading "Added" is a control lying about being one. A pack this
  account does not have is *read* rather than installed: the manifest lives behind
  the pack key and fetching it is the only way to know what a pack is called, which
  presage would only do as part of installing — so `preview_sticker_pack` is ours
  (`vendor/presage`), and a sticker from a pack you have never seen opens the same
  sheet as one of your own, with the title, the author, the count and every sticker
  in it. The one you clicked is marked in that grid rather than left out of it: a
  grid a sticker is missing from is a grid in an order that is not the pack's.
  Reading one is a CDN round trip *per sticker*, and a pack is routinely a hundred
  of them: fetched one after another that was half a minute of "Reading the pack…"
  for a sheet the phone opens at once, so `fetch_sticker_pack` runs sixteen at a
  time. The socket is a facade over an id-matched request channel and the CDN
  client pools its connections, so a clone per download is one more request in
  flight rather than a second connection. And the read is spawned off the worker's
  command loop rather than awaited in it — awaited there it stopped everything
  else, sends and receipts included, for as long as it took.
- **Maths.** `$…$` and `$$…$$`, read by `data::message::latex` and drawn as the
  symbols they spell: `\alpha` is α, `x^2` is x², `\frac{a}{b}` is a⁄b. There is no
  typesetting engine and there is not going to be one — what travels is the source
  the sender typed, because no other Signal client renders any of it, and a reading
  is the most that can be honest about that. A dollar is a currency symbol far more
  often than a delimiter, so a pair only counts with no line break and no space
  against either end: `$5 and $6` stays what it says. Set in the theme's serif,
  through the same family-override mechanism inline code uses — an integral sign in
  the interface font reads as a glyph somebody pasted rather than as an operator,
  and `typography.serif` exists for this and nothing else. Italics are for the
  *variables* and nothing else, which `latex::variables` decides off the rendered
  text: a single letter is a quantity, a run of them is a word (`sin`, `log`,
  whatever `\text{…}` held), lowercase Greek is slanted and uppercase is not, and
  digits, operators, brackets and ∑ are upright — which is how every book sets
  them. Slanting the whole equation, which is what this did, made it read as an
  italicised sentence that happened to contain symbols. `$$…$$`
  comes *out* of its paragraph and gets an element of its own, the way a fenced
  block does, and is set a third larger: a text run carries no size of its own in
  gpui, so an equation can only be bigger than the words around it by not being in
  the same run as them. Which is also why inline maths is the sentence's size and
  stays that way — the alternative is one element per span, and a line that wraps
  at every one of them. An accent
  is the one piece of structure Unicode can actually draw, as a combining mark, and
  only over a single character: over a group it lands on whichever glyph happens to
  be last, so it is left off instead.
- **Starting one.** `ui/new_chat.rs`, the forward picker's shape because the
  question is nearly the same one. What it adds is the two things the switcher
  cannot do: find somebody who is not in the contact list, and pick more than one
  of them. A round trip is only spent on a query shaped like an address — a name
  and a number is a username, a leading `+` a phone number — because everything
  else can only come back with nobody. A group asks for its members first and its
  name second, through the same `Prompt` every other single line of text goes
  through, and opens when the server has taken it rather than when the button was
  pressed: until then there is no thread to open.
- **The conversation column** is the whole of the space between the panels
  (`kit::column`). It was capped at a reading measure, the way prose is set; a
  chat log is not prose, and a window somebody made wide is a window they want
  used. Nothing is centred in it either way — centring moves every message
  sideways whenever a panel opens.

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
  media cache. `spawn` starts it; the caller never sees the thread. The store is
  one encrypted SQLite file holding all of it — every session key and every
  message ever received — so `store.rs` opens it through SQLCipher and `key.rs`
  keeps the passphrase where the platform keeps secrets.
- `crates/petunia` — the binary. `ui/` and everything gpui, `store.rs` (the one
  entity views observe, and the only way they talk back to the worker),
  `bridge.rs` (the worker's events into the store), `theme.rs`, `actions.rs`,
  `menus.rs`, `notify.rs`, `session.rs`.

`vendor/presage` is ours to edit, and two things here need it: the block list and
the nicknames both live in a contact's Storage Service record, which upstream has
no writer for. `update_contact` is the one read-modify-write behind both — a
manifest rewrite, retried once on a version conflict, which is what losing a race
with another device looks like. Neither path has ever run against a live account.

Whether somebody is **blocked** is not a question the server answers: it is the
`blocked` flag on that record, which every linked device reads out of the shared
manifest. So blocking is a write and a version bump, and the phone applies it when
it next reads the manifest. Dropping what a blocked person sends happens in
`Store::fragment`, the one funnel every arriving row goes through — dropped rather
than stored and hidden, since a filter over the history would be a filter every
view had to remember to apply. Both it and the nickname are offered by
right-clicking the conversation in the list, not only in the details panel: the
list is where you already are when you decide you have had enough of somebody,
and a panel you must open first to reach a verb is a verb behind a door. Blocking
asks first and is drawn as destructive; unblocking is the undo and asks nothing.
A group is offered neither, having nobody to name and nobody to block.

A **nickname** says so, in a line under the field. It is the one thing about one
that the field cannot show: it looks exactly like renaming somebody in a shared
address book and it is nothing of the kind — the record it goes in is encrypted to
this account, so it reaches your own devices and stops there. Said at the moment
of typing rather than in the details panel beside the name, which is where you
read it, not where you decide it. `Prompt::note` is that line, and it is drawn
only when asked for: a prompt with nothing to explain must not reserve a line for
one.

**Deleting a message** is two different things and the menu asks which. A remote
delete withdraws it from everybody and leaves a tombstone saying so; Signal
honours one from the message's author, within a day. "Delete for me" asks nothing
of anybody: the row goes from this device and a `SyncMessage.DeleteForMe` tells the
account's other devices to do the same, so it works on somebody else's message
where a remote delete cannot. No tombstone for that one — nobody was told
anything, so there is nothing for a line of text to report.

The macOS menu bar (`menus.rs`) is nothing but the actions the keymap already
dispatches, so a menu item and a keystroke are one code path and the shortcut
beside an item is read out of the bindings in force. An item with no action behind
it would be a menu that lies, so there are none. Quitting and hiding are the two
chords not taken from `config.toml`: a file that could rebind cmd+q could take it
away, and the item beside it would then name a key that does nothing.

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

`cmake` and `go` are there for one reason: resolving a phone number to an account
needs Signal's contact discovery service, which is presage's `cdsi` feature, which
reaches libsignal-net and through it Signal's fork of BoringSSL. The failure names
`boring-sys` and asks for cmake.

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
  at launch — and takes it back whenever an overlay closes
  (`Workspace::dismissed`, which every `Dismissed` handler goes through). A sheet
  that closes while the focus is still on the field inside it leaves the window
  with nothing focused at all, and every shortcut in the application stops
  answering: the symptom is a picker that works once, since choosing a result
  closes it by a route Escape never took.
- **A receipt is a network round trip.** Delivery receipts were sent from inside
  the receive loop and awaited there, so a backlog arrived at the speed of the
  receipts rather than of the stream — one round trip per message before the next
  was read, which is the whole of "petunia syncs slower than Signal". One receipt
  carries any number of timestamps, so behind a backlog `owe` collects them and
  sends one per sender when the queue drains; live there is a single message to
  answer and it goes at once, spawned off the loop rather than awaited in it. The
  drain is also `queue_signal.is_some()` — the same oneshot the send queue waits
  on — and the stream ending without it flushes anyway, since nothing will deliver
  those messages a second time to ask again.
- **Reading a message is having it in front of you.** Which is two conditions, not
  one: the conversation has to be the open one *and* the window has to be the one
  in front. Asked as "is this the active thread" alone, a message arriving while
  petunia sat behind a browser went unread nowhere and sent a read receipt back
  saying it had been seen. `Store::frontmost` is the window's answer, set by
  `observe_window_activation`, and it decides both the unread mark and the receipt
  — and the notification, which used to ask `window.is_window_active()` a second
  time for itself. Coming back to the front owes the receipts that went unowed.
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
  pixel size, so the fitted size is computed explicitly — and from the *file*
  (`image::shape`, memoized, a header read) rather than from the sender's
  declaration. The declaration is missing for anything of our own until the thread
  is reloaded, in which case the box is the whole 4:3 of `image_max_*`; and it
  disagrees with the bytes whenever EXIF is involved, since a phone stores a
  photograph landscape and Signal declares it rotated. Both show up as margin
  around the picture, because a contained image centres itself in the box it was
  given. A video's box comes from its poster for the same reason.
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
- **A crop is decided before the resample, not after.** `ObjectFit::Cover` over an
  image resampled to *fit* hands the GPU something smaller than the box in one axis
  and asks it to enlarge it — a wide photograph in a square thumbnail was then
  blurrier than the source it came from. `image::Fit` is part of the cache key, and
  `cropped` asks for the box to be filled.
- **gpui's asset cache never gives anything back.** `Window::use_asset` keeps a
  loaded asset in `App::loading_assets` for the life of the process and offers no
  eviction at all — and a resample is uncompressed BGRA, a megabyte or two per
  photograph per size it was drawn at, with an animation up to `MAX_FRAMES` of
  them. Reading back through a thread of pictures therefore only ever grew, which
  was the whole of "memory use is high". `image::Cache` holds them instead, keyed
  the same way, and drops the least recently drawn past `RESIDENT` — both the
  buffers and the atlas tile, which is a second copy on the GPU and is only
  reclaimed by `App::drop_image`. `RESIDENT` has to stay comfortably above what a
  window can show at once: evicting something on screen is asking to decode it
  again next frame, forever.
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
  with them; row zero is always present, even when it draws nothing, because a row
  that came and went would shift every index behind it.
- **A count cannot tell the list what changed.** It addresses its scroll position
  *and every height it has measured* by index, so `splice` has to name the range
  that was rewritten. "There are `n` more rows and they arrived at the front" is
  wrong whenever a page changes the rows it lands beside — which a page of older
  messages almost always does: the message that was the oldest had a day separator
  above it and a run header of its own, and once there are messages above it from
  the same person on the same day it has neither. Two rows go where twelve arrive,
  the count says eleven, and from then on every measured height belongs to a
  different row than the one it was measured from. That is the shove and the
  jitter when reading backwards. `group::changed` diffs the row lists instead —
  shift-invariantly, since a `Row` says which messages it draws by *position* and
  a page of older ones renumbers every row in the thread without changing what one
  of them draws. And `splice` only carries a scroll position that sits *after* the
  range: a reader inside it, or on row zero above it, is left at the top with the
  page they asked for below them, so `reconcile` puts them on the first row the
  rewrite did not touch.
- **A history is not a loaded history.** A message arriving live builds one out of
  nothing but itself, because `history_mut` creates the entry it is asked for. So
  "has this thread been opened before" cannot be "does it have a history":
  `activate` asked that, and a conversation talked in while petunia was closed
  then opened showing only the handful the receive queue had delivered, with
  nothing behind them and no way to scroll back — everything on disk invisible
  until a restart. `History::has_page` is the real question, set by `merge` and
  `prepend` and by nothing else.
- **Derived state is state you can lose.** The sidebar used to list a conversation
  only if a *preview* was in memory, and previews were rebuilt at every launch by
  scanning each thread's newest twenty stored rows and projecting them. A page of
  reactions, edits and tombstones projects to no message, so the scan produced no
  line, so `Entry::started` said false — and the person vanished from the list with
  their whole history still on disk and no way back to it. The same shape lost the
  names: `refresh_profiles` deleted the cached profiles and refetched them one
  round trip at a time over the whole address book, keeping nothing, so a launch
  that ended before the crawl did left everybody it had not reached as eight
  characters of their uuid — and the next launch started from nothing again. Both
  are written now the moment they are learned (`db/previews.rs`, `db/names.rs`,
  migration `007`) and read back before anything touches the network, so the
  network is what makes them *current* rather than what makes them exist. And
  `started` no longer rests on a line alone: `last_activity` is the bare fact that
  a message is there, which survives a line that could not be built —
  `Event::Activity` is that fact and nothing else.
- **One file, two pools, one key.** The store is encrypted with SQLCipher, which
  presage-store-sqlite already bundles; `key::passphrase` keeps a generated key in
  the platform's own secret store (the Keychain, the Secret Service) and nowhere
  else. petunia's own tables live in presage's database *file*, so `db::Db` opens
  the same path with a pool of its own — and a pool that forgets the `PRAGMA key`
  does not fail politely, it reports a corrupt database. `store::options` is the
  one place either of them is configured. A store written before any of this is
  plaintext, and `store::convert` migrates it on the first launch through
  `sqlcipher_export`, leaving the original beside it as `store.plaintext.bak`
  rather than deleting it. `ATTACH` inherits the connection's open flags, so the
  conversion needs `create_if_missing` for the file it is *writing*, not the one it
  is reading.
- **A sticker that fails once fails forever.** A pack is installed once and its
  hundred stickers fetched in one burst; whatever failed in that burst was stored
  with no bytes and never asked for again, so the tile drew nothing for as long as
  the pack was installed. Two answers, because either alone is not enough:
  `fetch_sticker` retries each download three times (`vendor/presage`), and
  `repair_sticker_packs` checks every installed pack against what is actually on
  disk at each launch and re-reads the incomplete ones — through
  `preview_sticker_pack`, not `install_sticker_pack`, since the pack is already
  installed and installing it again would tell every other device to do something
  they did months ago. And a pack nothing could be drawn of is *counted* rather
  than dropped: an empty picker saying "no sticker packs yet" to somebody with ten
  of them is a failure reporting itself as a fresh install.
- **Signal's servers do not know your username.** They hold only a *hash* of it,
  which is what makes a username lookup private. The plaintext lives in the
  account's Storage Service record and nowhere else, so a client that does not
  read it there cannot know its own — the symptom was settings reporting no
  username for an account that plainly had one. `refresh_username` reads it once
  at startup, and the `signal.me` link is rebuilt from the entropy and handle
  beside it rather than stored.
- **A username is a name and a number** joined by a dot — not `@name#1234`, which
  is Discord's. The discriminator is part of the identity, so two people may share
  a nickname; `reserve_username` offers the server one candidate when the caller
  typed a number and a spread of generated ones when they did not, because being
  handed another number than the one asked for is not a reservation anybody wants
  silently. Nothing in the interface explains any of this or prints a specimen
  username: a settings pane that teaches Signal's own vocabulary back to somebody
  already signed in is a paragraph nobody reads twice. `ui/username.rs` draws the
  two halves in *one* box (`kit::field`, which is the box rather than the input for
  exactly this reason) with the dot inside it, since two boxes with a dot floating
  between them read as two unrelated controls. Both halves are filtered on the way
  in — digits for the number, and lowercase letters, digits and underscores for the
  name — so the field shows the username that would actually be asked for rather
  than the capitals somebody typed. And the line under them is always drawn, empty
  or not: one that came and went moved the buttons out from under the pointer, and
  a "Set" that goes quiet without saying why is a control with no reason given.
- **A page of history is not a page of messages.** A reaction, an edit and a
  delete are stored rows that project onto a message already loaded, so a page can
  add nothing at all while the store still reports more behind it. Where the next
  page is asked from is therefore the oldest *row* a page reached
  (`Event::History::covered`), not the oldest message in it. Derived from the
  messages, the top of the list asks for the same page for as long as it is on
  screen — harmless while a button had to be pressed for it, a query per frame now
  that reaching the top is the request. Which is also why `covered` is read off the
  `ts` column in `db::messages::page` rather than off the decoded rows: a row that
  fails to decode is still a row the page reached, and `filter_map(decode)` drops
  it. A page whose rows all failed left `covered` at `None`, which moves the mark
  nowhere at all.
- **A visible row is re-rendered every frame**, so anything derived in one has to
  be cheap or cached. Two things here were not: `Theme::highlights` is a round
  trip through Zed's theme JSON (~260µs), now built once per theme install and
  read from the `Palette` global; and tree-sitter reparses a code block from
  scratch (~600µs), now memoized in `ui::message::content` on the code, the
  language and the theme. The sidebar is a scrolling `div` and so rebuilds every
  row of itself per frame, which makes the same rule the sidebar's: what a row
  needs is kept on the `Index::Preview` beside it — one clipped line, built when
  the message arrives — rather than summarised out of a whole `Message` per row per
  frame, which allocated a copy of every body in the list on every frame of every
  flick.
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
- **The text input brings its own padding.** `Input::small` insets its content by
  eight across and two down, which inside a card that has a padding of its own is
  that padding twice on one side and once on the other: the words started sixteen
  pixels from the left edge while the send button sat eight from the right, and the
  two sat two pixels apart vertically. The composer strips it (`px_0`, `py_0`) and
  lets the card hold the padding, one number for every edge.
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
  built on that. So does anything with a scroll of its own drawn over something
  that also scrolls — the settings select's list of themes, whose wheel otherwise
  moved the page behind it and took the list along.
- **An `Img` with no id never animates.** gpui keeps which frame a multi-frame
  image is showing in element state, and element state is keyed by the element's
  id — so an image without one is handed `None` for its state every frame, leaves
  the counter at zero, and draws frame one forever. That was every GIF in the
  application: decoded whole, resampled whole, frozen. `image::animated` is the
  call that gives one an id; `picture` deliberately does not, because an id has to
  be unique among its siblings and inventing one per avatar in a list is a
  collision waiting to happen.
- **A percentage max-width inside a shrink-to-fit column resolves against
  nothing.** A bubble is `max_w(relative(BUBBLE))` inside the side column, and the
  column has to be `flex_1`: sized to fit its content instead, the percentage
  resolves against a width that is itself waiting on the content, and every bubble
  collapses to its longest word and wraps there. That is the "bubbles are broken,
  they keep wrapping" symptom, and the fix is one call.
- **Two lines naming one action is not valid TOML.** `[keys]` keys on the *action*,
  so an action reachable by several chords cannot be written as a line each —
  `keys::written` groups by action and `write` emits a list for the ones with more
  than one. The reader takes either shape, and naming an action in the file
  replaces *every* chord it had rather than adding to them.
- **A highlight has no padding, and no corners.** `HighlightStyle` is a colour over
  a range of text, so inline code cannot be given the rounded box markdown draws it
  in — which left `` `bat` `` as a wash the shape of the letters. `ui::wash` is the
  answer: one text layout, unchanged, so a line still wraps as a line, with the
  boxes painted underneath it from the glyph positions that layout settled on.
  The padding is the box's, not the text's: `wash` inflates each rectangle by a
  fifth of an em, which is what widening the run with thin spaces used to buy at
  the cost of two characters nobody typed in every copy of the message. Discord's
  own measurements otherwise — three pixels of radius, and no border, because a
  hairline drawn around one word reads as a box somebody forgot to fill. The
  fenced block keeps its border and adds a bar across the top: the language on
  the left, and on the right the one thing anybody wants from somebody else's
  listing, which is to have it. Which is why `box_of_code` holds no padding of
  its own — a bar has to reach both edges, so what goes inside pads itself.

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
