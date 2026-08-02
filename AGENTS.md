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
  compiled in, and so does `signal-dark` — Signal's own near-black and
  ultramarine, for somebody who wants petunia to look like the client they came
  from. Hand-written rather than converted, since there is no Zed theme behind
  it, and the one shipped theme that names its `accents`: Signal's conversation
  colours are part of what it looks like.

## Layout

Follow the reference design, not the layout the old iced client used. The iced
version is gone and its arrangement is not a precedent for anything.

- **Sidebar.** Runs the full height of the window, with the traffic lights
  floating over its own top padding — a band that exists *for* them, so it
  collapses where there are none: off macOS, and in fullscreen, where macOS takes
  them away. Small quiet section headers and two-line entries carrying real
  metadata — a face at Signal's own size, which is taller than the two lines beside
  it, the count in the accent on a filled pill rather than as one more grey
  annotation, and the ticks for the last thing *you* said where Signal puts them:
  on the line, not only in the conversation. Only our own messages carry a status
  at all, so `Preview::status` being `Some` is also the answer to "was this mine",
  and there is nothing to draw on anybody else's row. The badge is always the
  number now: the dot that stood for a single unread said less than it cost, since
  the row already reports that something is new by setting the name in the emphasis
  weight — a name is the row's headline and is set in one (Semibold, as AppKit
  sets the title line of any two-line row), so what unread changes is the colour
  rather than the weight; a column that restyles a line every time a message lands
  has two typefaces in it. The one you are *in* is a filled row and nothing else
  (`kit::row`): Signal's own grey, a clear step above the hover. An accent tint
  with a hairline and a stripe down the edge was legible from across the room and
  read as a selected *cell* — the eye finds one filled row among unfilled ones
  without being shouted at. What the fill does change is the preview under the
  name: the quietest grey in a theme was chosen to sit on the list's background
  rather than on a fill, and on the fill it was a line you could see was there and
  not read — worst in light, where two of those greys were mirrored straight off
  the dark theme's. So the row you are in steps its second line up exactly as an
  unread one does, and petunia's own `light` is lettered darker than its `dark` is
  lettered light. Rows are given room to be read down rather than
  across: `LIST_PADDING` and the padding inside a row are what make the column
  scannable, and they were set for a table of contents.
  Above the list, one box that searches (`Sidebar::filter`): it narrows the list
  to the names that match *and* asks the store the same question `cmd-f` asks, so
  what was said appears under a **Messages** heading below the conversations and
  opens where it was said. One search in the application rather than two that can
  disagree — picking a result goes through the workspace's `reveal_hit`, which is
  the sheet's own path. Above the list rather than over it, because a field that
  scrolled away with the rows is a field you have to go back for. Return opens the
  top of whichever half answered. Not on
  the rail, which has no room for a line of text — so what was typed before it
  collapsed narrows nothing while it is one. Identity sits at the very bottom: the
  display name and the connection, at a face smaller than the list's, since that
  picture is a label on a line and at the rows' size it was the largest thing in
  the column — the account shouting over every conversation in it. The presence
  dot is sized off that face rather than the list's for the same reason: it
  annotates a name here, where in a list it badges a control. And nothing
  that changes under the pointer — hovering it used to swap the name
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
- **Voice messages.** A microphone beside the send, and while one is running the
  card becomes the recording rather than growing a strip: a light that breathes
  with the level, the clock, the shape of what has been said filling from the
  right, and the two things that can happen to it. Beside the send rather than in
  place of it — Signal swaps the two depending on whether the field is empty,
  which means the control under the pointer changes what it does as you type.
  What is written is the microphone as it arrived: sixteen-bit PCM at the
  device's own rate, in a WAV (`crates/media/src/recorder.rs`). Signal's own
  clients send about thirty-two kilobits of AAC, which is enough for a sentence
  in a quiet room and audibly not enough for anything else; this is a megabyte a
  minute against their quarter of one, and it is the recording rather than a
  guess at it. Sixteen-bit specifically, not the thirty-two-bit float rodio's own
  writer emits: what is on the other end is a phone, and a phone's decoder knows
  the format every recorder has written since 1991. The waveform is gathered
  while recording, in the same measurement `waveform::shape` reads a stored file
  as, so a note recorded here and one from a phone are drawn from the same
  numbers. `voice_note` on the spec is what makes it a voice note on the wire —
  the content type is not, and every other client reads the flag.
- **Playing faster.** The ladder is Signal's — 1×, 1.5×, 2× — and what it changes
  is how long the recording takes rather than what it sounds like.
  `media::stretch` is WSOLA: the sound is cut into overlapping grains and the
  grains are laid back down closer together, so each keeps the periods it was cut
  from and only the timeline moves. rodio's own `set_speed` is a *resample* — it
  reports a higher sample rate for the same samples — which is a voice note an
  octave up, and a speed control is for getting through the words rather than for
  making somebody sound like a cartoon. Laying grains down blindly is an echo,
  since two a hop apart are rarely in phase; the search either side of each
  grain's nominal position is what repairs that, and a pitch period is a few
  milliseconds against a ten-millisecond search. The speed is shared with the
  source rather than baked into it, so changing it is a change to the next
  twenty-five milliseconds and to nothing already written, and at 1× the search
  finds the offset it was taken from and the complementary fades reconstruct it
  exactly — one path through the code rather than two that could sound different
  from each other. The playhead is the stretcher's own count of the frames it has
  *read*: rodio counts what it has written, which at 2× is half the answer.
- **Sounds.** A short tone when a message goes and another when one lands
  (`audio::Chime`), synthesised rather than shipped: two sine partials under an
  exponential decay is a struck bell, which is what a notification has sounded
  like since there were notifications, and it is thirty lines instead of two
  assets to carry and license. Its own voice on the mixer, so it lands over a
  voice note rather than stopping it. Deliberately quiet — this plays every time
  a message arrives, and the sound that survives that is the one you stop hearing
  rather than the one you turn off. The same mute and group rules a banner obeys,
  through the same predicate (`notify::worth_telling`), because a sound *is* a
  notification with no words in it and two policies that can disagree is a
  conversation that is muted for one of them. Unlike a banner it does not care
  whether the thread is on screen: "did that send" is a question you have with
  the conversation in front of you.
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
  decides what counts as text and reads the head of it once. Which is why it is
  drawn in the very box a pasted listing gets, `content::bar_of_code` across the
  top and all: the two are one object arriving two ways, and the name floating on
  a line above a box was a second header for it. What the bar carries is the whole
  difference — an icon, the file's name and its size where a listing has the
  language it is in, and a button that saves it where a listing has one that
  copies it. An audio file that
  turns out to be a *record* — `crates/media/src/song.rs` reads its tags — is drawn
  as one: the cover — rounded on the picture as well as on the well behind it,
  since `overflow_hidden` clips a child to the parent's rectangle rather than to
  its corners — the title, who made it, and the bit depth and sampling rate,
  with a bar that fills where the waveform would be. A waveform is Signal's shape
  for a voice note and there is none for a track, so drawing one is forty-four grey
  bars saying nothing where the cover belongs. Everything else is the chip in
  `ui/message/media.rs`.
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
  and Signal draws neither in one. Where the *receipt* goes is the layout's too
  (`Run::marked`): a bubble and an IRC line each carry their own, which is what
  the client each shape is borrowed from does, but a run of six things you said
  under one header got six ticks down its side saying the same thing six times.
  So the standard layout does what Cinny does with Matrix's receipts and marks
  the furthest one only — the newest message of yours in the thread — since
  everything above it got at least that far by definition. "Edited" is not a
  receipt and is still drawn wherever it applies. And a copy says so
  (`Conversation::copy`): a clipboard write is invisible, so for a moment the
  message lights in the accent and the copy button becomes a check, and the bar
  stays up while it does — an answer that appears under a pointer that has moved
  on is an answer to nobody.
- **Spoilers.** A rounded block painted *over* the words (`ui::wash`'s `covering`,
  keyed in `ui::spoiler`), lifted by a click. Over rather than instead of: the
  text is laid out as it was written, so uncovering one is a repaint rather than a
  reflow and every offset a selection or a highlight holds stays where it was.
  What was there before was a run of `█` in place of the words, which had no
  corners to round, clamped at forty characters, and had nothing to click at all
  — a spoiler that could not be revealed is a message petunia does not show. The
  block is the theme's muted grey at full opacity, because one you can read
  through is not one. A spoiler inside a quote is covered too, and named after the
  message it quotes rather than the one it is in.
- **Replies.** Three shapes for the message being answered, beside the layout
  under *Appearance* and for the same reason: how much room the context takes is
  what a reply looks like, not what it says. **Signal**, the default, is Signal's
  own — a rounded block nested in the message that answers it, with the thumbnail
  at the far end; a block, because a quote is not an annotation but a second
  message drawn inside the first, and round on every side because what it sits in
  is round too. Filled in the theme's own quiet grey and lettered in the theme's
  own text, which is the one thing about Signal's shape not worth copying: they
  fill it with the conversation's colour, and a hue generated per person puts a
  different bright rectangle in the middle of the thread for every person quoted,
  which is a lot of noise for the part of a reply nobody reads first. The name at
  the top is what says whose words these are — in every one of the three shapes,
  and in none of them in their colour;
  **bar**, petunia's original, a hairline with the quote beside it, context kept
  as light as context can be drawn; and **line**, the reply mark, who, and what
  they said on one truncated row, which is the least a quote can be and still be
  one. All three are `content::Quoted`, resolved once so they cannot disagree
  about what was actually quoted.
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
- **Maths.** `$…$` and `$$…$$`, read by `data::message::latex` into a *tree*
  (`latex::parse`) and written back out as one line by `latex::flatten`, which is
  the same grammar read twice rather than two readers that can disagree. A
  display equation gets the tree: `ui/message/maths.rs` lays it out with real
  boxes — a numerator over a denominator with a rule between them, limits above
  and below a ∑, a radical with a bar over what is under it, scripts set smaller
  and raised, and a `\left…\right` pair stretched to what it holds. An equation
  *inside a sentence* gets the line — `\frac{a}{b}` as a⁄b — because a wrapping
  line has to be one text run and gpui has no inline box, which is the same split
  TeX itself draws and the same reason most houses set an inline fraction on a
  slash. There is still no maths font and no glyph variants, so an integral sign
  is the size it is; what is drawn is the structure, and the glyphs are Unicode's.
  What travels is the source the sender typed, because no other Signal client
  renders any of it. A dollar is a currency symbol far more
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
- **The viewer** (`ui/viewer.rs`) takes the whole window, as Signal's own does.
  It was a sheet inset from every edge with the conversation dimmed behind it,
  which sounds like context and reads as a picture in a smaller window: the thing
  being looked at is a photograph, and what a photograph wants is the screen.
  Covering the window means the strip of controls across the top is what the
  traffic lights float over, so it clears them the way the header does when the
  sidebar is away. Clicking beside the picture still closes it. The rail of
  thumbnails underneath is a row of *wells* rather than a row of pictures:
  `ObjectFit::Cover` fills the box and lets the long axis hang over, and nothing
  clips that but a parent that says so — so a portrait photograph grew out of the
  rail and over its neighbours. The gestures are the platform's: a pinch zooms, a
  two-finger scroll pans while there is anywhere to pan to and steps the reel
  when there is not, and a mouse wheel zooms, because a wheel has one axis and in
  a viewer it means closer. Zoom is anchored on whatever is under the pointer —
  scaled about the centre, the detail somebody leaned in to look at slides off
  the stage on the way in, and the gesture becomes zoom, hunt, pan, zoom again.
  The picture is `absolute` so that being larger than the stage is not a request
  for room from the very box the zoom is measured against, and the pan is bounded
  by however much of it hangs over each edge: a picture flicked off the stage
  entirely is a black screen with no clue that the way back is a keystroke.
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

`vendor/presage` is ours to edit, and three things here need it. The block list
and the nicknames both live in a contact's Storage Service record, which upstream
has no writer for: `update_contact` is the one read-modify-write behind both — a
manifest rewrite, retried once on a version conflict, which is what losing a race
with another device looks like. Neither path has ever run against a live account.
The third is `get_attachment_reporting`, which is upstream's own download with the
byte count let out of it: `get_attachment` reads the whole stream in one call, so
there is nothing to report until there is nothing left to report, which is a
progress bar that can only slide.

**Disappearing messages** are set from the conversation's own menu, on the ladder
Signal offers rather than as a switch — "on" is not a setting anybody can act on
without also being asked how long. Turning one on *is* the message: there is
nowhere on the server a one-to-one timer is kept, so a client that does not write
the update down forgets it the moment the window closes, which is what
`petunia_expire_timers` is for and why a group's own record only seeds it. When
the clock starts is Signal's rule: the moment a message is sent, for our own, and
the moment it is *read*, for everybody else's — so nothing vanishes out of a
conversation nobody opened, and `Command::StartExpiry` comes from the window,
which is the only thing that knows a message has been seen. `sweep_expired` runs
every half minute and deletes locally and only locally: everybody in the
conversation has the same timer and is running the same clock, and asking them to
delete something they are already deleting is a message per message per
recipient. The conversation says so where it is being read — a chip in the header
that opens the same menu — and each message that is going away carries a small
clock rather than a countdown, because a second-by-second number on every message
in a thread is a thread that never stops repainting.

**How much has been said** is counted rather than kept (`db::tallies`), behind a
preference that is off: a running total written as messages arrive would be a
second copy of the truth going stale the first time a row was deleted from under
it. It is a full pass over the store, so it is asked for when the details panel
opens and again when it moves to another conversation, which is when the numbers
are being read. A receipt, a reaction and a tombstone are rows and are not
counted — "you have sent four thousand messages" with the thumbs-up in it is not
the number anybody meant — and `classify` is the same reader the conversation is
built from, so the two cannot disagree about what a message is.

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

**On Linux** the shell carries the system libraries as well, because nothing on
that platform is simply there the way a framework is: `pkg-config` for the `-sys`
crates to ask, `openssl` and `dbus`, gpui's wayland, xkbcommon, Vulkan loader, X
libraries, fontconfig and freetype, and `alsa-lib` for cpal. Wayland, the Vulkan
loader and the X libraries are opened *by name* at run time rather than linked,
so `LD_LIBRARY_PATH` is set as well — without it the binary builds and then
cannot find a surface to draw on.

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
  the same way, and drops the least recently drawn past `BUDGET` — both the
  buffers and the atlas tile, which is a second copy on the GPU and is only
  reclaimed by `App::drop_image`. `BUDGET` has to stay comfortably above what a
  window can show at once: evicting something on screen is asking to decode it
  again next frame, forever.
- **A size is a cache key, and a drag changes it every frame.** A request is keyed
  by the device pixels an element occupies, so dragging the window wider asked for
  a size that had never been decoded on every frame of the drag — and an element
  handed a decode that has not finished draws *nothing*. That is every picture in
  the window blinking out and back for as long as the resize lasts. Two answers,
  because either alone leaves half of it: `image::step` rounds a request up a
  sixteenth at a time, so a drag across the screen asks for a dozen sizes rather
  than a thousand and the picture is at most six per cent larger than its box —
  which the GPU takes out again in the one bilinear tap it was always going to
  make; and `Cache::nearest` answers a request still being decoded with the same
  picture at the closest size that is, scaled for a frame or two rather than
  absent. The stand-in is marked as drawn when it is handed over, or it is evicted
  for being old while it is the only thing on screen.
- **A cache counted in entries is a cache for one size of thing.** The ceiling was
  a count, and a photograph and a sticker tile are three orders of magnitude apart
  — so a number generous for a page of photographs was a number the sticker picker
  blew through in one frame, asking for a thousand entries against a couple of
  hundred and evicting every avatar in the window to get them. Each was then asked
  for again on the next frame and evicted again on the one after: *"opening the
  stickers makes every picture in the application disappear"*. The budget is bytes.
  And a picker tile is drawn with `image::picture` rather than `image::animated`,
  because `Kind::Animated` is the only request that decodes past the first frame
  and a grid of a hundred stickers at `MAX_FRAMES` apiece is a hundredfold of the
  memory for something nothing but an id could ever play anyway. Cover art is the
  third kind: it lives *inside* an audio file, so it is a `Kind` on the request
  rather than a second cache or a file written out beside the audio.
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
- **A message you wrote is one you are looking for on screen.** The list keeps a
  reader where they are when something arrives at the back, which is right for
  everybody else's messages and wrong for your own: sending from halfway up a
  thread left the message somewhere below the window. `Conversation::follow_own`
  is the one exception, and it is keyed on *which* message rather than on
  whether the newest one is ours — that stays true for every frame after a send,
  and a rule that scrolls to the end on all of them is a thread that cannot be
  scrolled back through at all.
- **A message you sent has nobody to read it.** Signal starts the clock on a
  disappearing message when it is *sent*, for our own, and when it is *read*, for
  everybody else's — so `expiring` both stamps the field and writes the deadline,
  and without the second half everything this account said stayed in the
  conversation forever while vanishing out of every other one. The field is the
  other half of the same trap: it is what makes a message disappear for the
  recipient, and a send path that skips it is a conversation with the timer on
  where only one side's messages go. Every path that puts something in a thread
  needs it — text, attachments, stickers, polls, and the edit that replaces one.
- **`activate` asks before the answer can exist.** Reading a conversation is what
  starts its messages disappearing, and `activate` asked the history for them in
  the same breath as it requested the page — so on a thread nothing had opened
  before there was nothing in memory to start, and a conversation full of
  disappearing messages sat there with not one of them counting down. The page
  arriving is the moment, which is the same place the first open's read receipts
  are owed from.
- **A line outlives the message it was taken from.** The sidebar's preview is
  written when a message arrives rather than derived per frame, so nothing
  revisits it — and a disappearing message that has gone from the thread and is
  still legible in the column beside it has not disappeared. `forget_preview`
  clears the line when it is of *that* message and leaves `last_activity` alone,
  so the conversation keeps its place rather than vanishing with the message;
  what replaces it is whatever the store now says is newest (`reline`), because
  `touch` refuses to go backwards and the replacement is older by definition.
- **A read sync names an author and a timestamp, and no thread.** Which thread a
  `SyncMessage.Read` is about is the receiver's to work out, and taking the
  author's own one-to-one for it is right for exactly half of them: every group
  message read on the phone cleared nothing here and moved the watermark on an
  unrelated conversation with that person — where, being a watermark, it then hid
  real unread messages behind it. presage's store already answers the question
  (`thread_for_sender_and_timestamp`); the author's thread is the fallback for a
  message this device never received. And the mark is a watermark rather than the
  whole thread, so what follows it is recounted rather than cleared: reading
  something from this morning on the phone does not mean everything since has
  been seen.
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
  beside it rather than stored. The link itself is a *control* rather than a
  line: forty characters of base64 printed under the username was the one string
  in the settings pane nobody reads and the one string everybody needs to send,
  so it is copied or shown as a code somebody else's phone can point a camera at
  (`kit::qr`, shared with the linking screen). Black on white whatever the theme
  is, and with the quiet zone the standard asks for — a scanner is looking for a
  printed mark, not for something that matches the window it is in. Folded away
  until it is asked for, because it is two hundred pixels tall and the account
  pane is not mostly about being scanned.
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
- **There is no Medium on macOS, and the ladder is what carries the hierarchy.**
  The obvious repair for the note below — ask for 510 rather than 500, since CSS
  matching walks *upward* first above 500 and Medium is reported at 530 — does not
  work either: it finds Semibold at 600 and sets the entire application in it.
  Which is what "everything is bold" looks like, and it is also the wrong idea.
  The Human Interface Guidelines set body text, list rows, labels and control
  titles in Regular and spend weight on *hierarchy*: Semibold is a headline. So
  `kit::BODY` is Regular, applied once at the root, and the things that are
  headlines say so — the conversation title in the header, the name on a sidebar
  row, the author of a quote. Everything below Semibold does its work with size
  and colour instead, and the smallest size in the window is eleven, which is the
  Guidelines' footnote; ten was a size nothing else used and nobody could read.
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
  The padding is the box's, not the text's: `wash` inflates each rectangle rather
  than widening the run with thin spaces, which is what that used to take — two
  characters nobody typed, in every copy of the message. What it paints is the
  *same object the fenced block is*, only the size of a word: the block's fill,
  the block's hairline, and half the block's radius, since the full one on a box
  a single line tall is a lozenge rather than a chip. Filled and unbordered —
  which is what this was — `` `bat` `` read as a highlighter pen that happened to
  be monospace rather than as code, and the point of marking it at all is that it
  is the same kind of thing as the block. The block adds a bar across the top:
  the language on the left, and on the right the one thing anybody wants from
  somebody else's listing, which is to have it. Which is why `box_of_code` holds
  no padding of its own — a bar has to reach both edges, so what goes inside pads
  itself, and `bar_of_code` is that bar, shared with the text attachment.
- **A box arrived at by arithmetic is a box that is wrong.** The viewer resamples
  a picture for the stage, and the stage's size was the panel's fraction of the
  window less a *guess* at what the strip above it, the transport and the rail
  come to. Every one of those was a few pixels out, and too generous by any of
  them is a picture drawn taller than the stage — clipped at the bottom by the
  stage's own `overflow_hidden`, which on screen is indistinguishable from
  something covering it. Which is the same lesson the waveform's `canvas` teaches:
  a layout closure does not know what it ended up as, so `Viewer::stage` records
  the bounds and `box_for_the_picture` reads them, falling back to the arithmetic
  for the one frame there is nothing yet to read.
- **A viewer's keys are the viewer's.** `left`, `space` and a bare `0` are what
  every picture viewer binds, and three of them are already spoken for by the
  conversation — `up` and `down` scroll it. So `actions::viewer_bindings` is
  scoped to `VIEWER_CONTEXT` and the panel declares it with `key_context`, which
  is how the same key means one thing over a picture and another behind it. Fixed
  rather than configurable, like `cmd-q`: a mode that lasts as long as one picture
  is not worth twelve lines in `config.toml`.
- **A font family override only reaches a run a highlight has cut.** gpui applies
  `with_font_family_overrides` to whole `TextRun`s, and the runs are built from the
  *highlights* — so a span with nothing to highlight is a span the override slides
  straight past. Inline code has nothing: it is a family and a box and no colour,
  deliberately. So `` `bat` `` was named as monospace over a range no run ever
  lined up with, and came out in the body font with a faint box behind it. `cut` is
  the answer — a highlight that may well be empty, present only to start a run —
  and the maths serif needed exactly the same thing for the upright stretches of an
  unstyled equation. The overrides also have to arrive **sorted**, which two
  independent sources are not between themselves.
- **A receipt is a row in a thread you have never used.** presage saves receipts,
  and saves them under `Thread::Contact(sender)` whatever the message they
  acknowledge was in — a group message read by ten people is ten rows in ten
  one-to-one threads. So "does this thread have rows" is not "is there a
  conversation here", and asking it that way listed every member of every shared
  group in the sidebar with nothing to read and no history behind them.
  `data::conversational` is the real question, and `recent_rows` counts nothing
  else. A row that will not *decode* still counts: unknowable is not
  uninteresting.
- **Two authenticated websockets are one too many.** presage keeps one and hands it
  out, except to `receive_messages`, which needs an unused socket because one
  already carrying requests cannot also carry the stream. So a socket opened by a
  startup crawl is a socket the stream refuses, and the stream opens a second —
  two connections as the same device, which Signal answers with `4409 Connected
  elsewhere`. The loser reconnects, kicks the winner, and the two fight at the
  reconnect interval forever, with nothing but petunia at either end. `session`
  therefore starts the stream *before* anything else touches the network, and
  `receive` backs off rather than retrying on a fixed five seconds — a fixed
  interval is what keeps a fight going at that interval.
- **A reconnect handed the old socket is not a reconnect.** A websocket whose peer
  stopped listening without saying so — the machine slept, the network changed —
  does not fail: it sits there open on this side until a keep-alive goes
  unanswered, which libsignal-service notices a minute or two later. presage's
  `identified_websocket` hands the cached socket to whoever asks as long as it is
  not *closed*, so `receive_messages` was routinely given that corpse, the stream
  it built ended the moment it was read, and the loop reported itself as
  reconnecting every few seconds while delivering nothing. Which is the whole of
  *"the connection keeps flickering and messages arrive minutes late"*.
  `fresh_identified_websocket` (`vendor/presage`) opens a new one for the stream
  and drops the cached handle, and dropping it is what closes the old one: the
  process behind a socket ends when the last sender for its request channel goes.
  And rather than wait to be told, `watch_for_sleep` notices the sleep itself —
  `Instant` does not advance while the machine is asleep and the wall clock does,
  so a gap between the two is a sleep and a reason to throw the socket away at
  once. A network change with no sleep behind it is still the keep-alive's to
  find.
- **A typing indicator is not a data message.** presage's
  `Thread::try_from(&Content)` reads a group out of a `DataMessage` and answers
  `Contact(sender)` for everything else, so every group's dots were filed under
  the sender's one-to-one thread: beside the wrong name in the list, and never in
  the group they were typed in. A `TypingMessage` names its group by the
  *identifier* derived from the master key, and that derivation runs one way only,
  so `typing_thread` derives it for every group this account is in and matches —
  cached for the life of the stream, rebuilt when an id is not in it, which is
  what a group joined since it started looks like.
- **macOS asks who is asking, with a file chooser.** `notify-rust`'s backend needs
  a bundle identifier and looks one up by name when it has not been given one; the
  name it looks up is the literal string `use_default`, nothing is called that, and
  an unresolvable application reference opens the "Choose Application" panel. Every
  notification therefore put a chooser on screen naming a document nobody has ever
  had — *"a file chooser keeps appearing at random"*. `notify::name_the_application`
  hands over this process's own identifier once, before any notification is posted.
- **A cached attachment has no name.** It is content-addressed: a digest, with
  whatever extension the *declared* content type happened to name — and Signal
  declares source code `application/octet-stream`, which names nothing, and used to
  name nothing for FLAC and AIFF either. Everything that read a file by its path
  therefore worked while the message was being sent, where the path is still the
  file we picked, and stopped the moment the thread was reloaded out of the cache:
  a listing went back to being a chip (`text::language_of` asks the declared file
  name first, and the path second), and a record went back to being forty-four grey
  bars, because `Probe::open` reads the format off the extension and errors outright
  when there is none — `song::probe` guesses it from the bytes instead.
- **A decoder does not know how long a track is.** `Source::total_duration` is
  `None` for mp3, flac and ogg, which is most of what anybody attaches: the bar
  never filled and a click on it seeked nowhere, since a fraction of an unknown
  length is nothing. The tags know, having read the stream's own properties, so
  `audio::toggle` falls back to them.
- **A receipt has to reach the list as well as the conversation.** The sidebar is
  not built from the histories — a thread nobody has opened has none — so a status
  lives on the line too (`Index::apply_status`, and the `status` column of
  `petunia_preview`). Which it has to be *given*: `project` reads presage's store,
  which knows nothing of receipts, and the real aggregate is only resolved while a
  page is loading, because it needs the recipient count and therefore the manager.
  So the startup scan resolves what it can from disk and a group is counted as
  having more recipients than have reported — sent rather than read, since claiming
  delivery on one member's receipt is the one answer that would be a lie.
- **A voice note is marked, a record is read.** Signal has one kind of attached
  sound and draws it one way, which is right for somebody talking and wrong for an
  album track: forty-four identical grey bars where the cover belongs, and none of
  the title, the artist or the numbers that say what was kept. `media::song` reads
  the tags (`lofty`) and `Song::is_a_record` decides — a title *and* somebody named,
  because every phone recording carries a filename in its title tag. `voice_note`
  still wins outright: a mark from the sender outranks anything guessed from bytes.
  And the tags are read once per file, not per frame, for the same reason the text
  preview is.
- **Only Signal's own clients send a waveform.** The protocol carries one beside a
  voice note, and everything else — a note recorded elsewhere, an `.m4a` somebody
  attached, and every voice note petunia itself has sent — arrives with the field
  empty and got `audio::bars`' flat fallback: forty-four identical grey bars, which
  is a picture of nothing where the sound should be. `media::waveform` reads it out
  of the file instead, into the same array of bytes the protocol would have carried
  so nothing downstream knows which it got. Peaks gathered per chunk while decoding
  and resampled at the end, because no format anybody attaches stores a sample
  count; normalised, because a voice note is recorded at whatever level the room
  agreed on and against an absolute scale a quiet one is a flat line; and
  square-rooted, because amplitude is not loudness. What the sender sent still wins
  outright — it is what every other client is drawing for that message. It is a
  *decode*, asked for from a render, so it is cached per file like the tags and
  skipped above `waveform::LIMIT`, which is set where a voice note stops and an
  album side begins.
- **A strip of bars is not forty-four divs.** Each `flex_1` with a margin of a
  pixel is forty-four boxes whose widths are whatever is left after the layout has
  rounded each of them to the device grid — and at the width a message actually
  gives one, a couple of pixels apiece, that is bars in two widths with gaps that
  come and go. The record's bar had the matching failure at the other extreme: a
  three-pixel rounded rail with a percentage-width fill inside it, which is one
  rounded box clipping another and reads as a bar that is broken rather than empty.
  Both are painted now (`media::wave`, `media::rail`) from the bounds the `canvas`
  measured, which is the only number in the frame that is not a guess — as many
  bars as fit at a fixed size, spaced across the whole of it, and a knob on the
  rail so it reads as something that can be moved rather than only watched.
- **A theme may point two tokens at one colour.** `signal-dark` sets
  `border_focus` to the accent, which is perfectly reasonable and made every
  transport in the application look full: the unplayed track and the played fill
  were the same blue, so a voice note was a solid block and a record's bar was at
  100% before it had been touched. `media::unplayed` is the accent worn thin — a
  difference of *alpha*, which no theme can collapse — and anything else drawing a
  "not yet" against a "so far" wants the same treatment.
- **A record is a player, so it is laid out like one.** The card follows Apple's
  transport (`media::record`): artwork with a hairline, title and credit beside
  it, and across the bottom a round play button, the bar, and the times *under*
  the bar at either end — elapsed left, remaining right, signed. One clock at the
  end of the row read as a duration when stopped and as a position when playing,
  said nothing about what was left, and squeezed the bar into what was left over.
- **A picture in the viewer is sized to itself.** Handed the whole stage as its
  box, `ObjectFit::Contain` centres it correctly and to the pixel — and a portrait
  photograph then reads as a small picture adrift in a large black panel, because
  nothing on screen says where the box ends. `viewer::filling` gives it its own
  shape at the largest the stage allows. Not `media::fit`, which caps its scale at
  one: refusing to enlarge is right in a message and wrong in the one place whose
  entire purpose is to show a picture as large as the window can.
- **A grid of pictures is a grid of *boxes*.** An image is only square once it has
  decoded, so a shared-media grid built out of the pictures themselves settles
  into line a tile at a time and keeps whatever shape the odd failure leaves
  behind. The square is the well the picture sits in (`details::SHARED_TILE`),
  with `overflow_hidden` and a fill, and every cell is square from the first
  frame — and rounded on the *picture* as well as on the well, since
  `overflow_hidden` clips a child to the parent's rectangle rather than to its
  corners and a square image in a rounded well otherwise keeps its own corners.
- **The chrome has its own icons.** The library's set is hairline and
  square-ended, which beside a column of rounded cards reads as a toolbar borrowed
  from another application: `assets/icons` now holds the handful the chrome
  actually shows — search, compose, settings, plus, send, close — at the same
  Lucide geometry with round caps and joins. And the send arrow was the character
  `↑`, set in whatever the interface font made of it — the one mark in the window
  not drawn like the rest.
- **A receipt mark is a circled check.** Which is Signal's own alphabet and not
  Lucide's: one ring with a check in it sent, a second ring beside it delivered,
  and both rings *filled* read. Bare ticks in three colours were petunia's
  invention, and the colour was carrying what the shape should — Signal draws all
  three in the same grey and lets the fill say which is which, so `kit::receipt`
  does too. The pair only reads as a pair because the ring in front knocks a gap
  out of the one behind, and that gap cannot be had by drawing one glyph twice: it
  is a `clipPath` in `receipt-delivered.svg`, and the checks in the read mark are
  holes knocked out of filled discs with `fill-rule="evenodd"`. gpui rasterises an
  SVG to an alpha mask and tints it, so a hole is simply a hole and a two-colour
  icon is not available at all. The doubles are an eighteen-by-twelve box, so
  anything asking for a square squashes them.
- **Selecting words in a message is petunia's own.** gpui has no selectable text
  outside its own input, and the widget library's `TextView` selection reaches only
  its own views: a message is a laid-out run of glyphs. `ui::selection` holds the
  one selection there is — a range in one run — in a global, because the element
  that paints the wash and the element that hears the mouse are the same element,
  rebuilt every frame, with nowhere of its own to keep anything. `ui::wash` grew
  the drawing and the mouse handling, since it already had the layout and already
  paints boxes underneath text. The listeners are the *window's*: a drag leaves the
  paragraph it started in almost at once, and a `div` only hears a mouse move while
  the pointer is over it. Signal's behaviour, which is the browser's — a drag takes
  what it crosses, a double click a word, a third click the paragraph — and `cmd-c`
  copies it. That chord is bound with no context, so the deeper bindings win first:
  the library's `Root` tries its own copy and passes it on when it has nothing, and
  a field's copy is deeper still. Clearing is a *captured* mouse down on the
  conversation, which runs before the words hear the click; as two bubble handlers
  it would come down to which was painted last.
- **A sticker is a cut-out.** The sheet used to sit each one on a sunken square so
  it would have an edge; what that actually draws is a grid of boxes with pictures
  in them. Nothing behind them.
- **`Decoder::new` builds a decoder that cannot seek.** rodio's `new` takes a
  reader and is told nothing else; `Decoder::try_from(File)` reads the length off
  the file's own metadata and marks the stream seekable. Without both, `try_seek`
  refuses outright and symphonia cannot work out the length of a format that does
  not store one — which is mp3, ogg and every voice note Signal sends. That is
  the whole of "clicking the waveform does nothing", and it is also why the bar
  never filled: the fallback to the tags was covering for a decoder that had been
  built wrong.
- **rodio counts the position in the sped-up timeline.** `Speed` works by
  reporting a higher sample rate and `track_position` wraps it, so at double speed
  `get_pos` returns half the real position and `try_seek` takes half the real
  target — and *changing* the speed rescales every sample already counted, which
  is a playhead that jumps. `audio::elapsed` and `audio::scaled` are the one
  boundary where the two clocks are converted, and `set_speed` re-seeks to where
  it was so the count starts again against the new rate.
- **A group's hover is its own rectangle's.** `group_hover` asks
  `GroupHitboxes::get(..).is_hovered`, which is the group element's bounds and
  nothing else — so a control positioned *outside* those bounds is visible only
  while the pointer is somewhere it is not. The message toolbar hangs above the
  message by half its own height, and reaching for it took the pointer out of the
  message, which hid the thing being reached for: the top half of every button in
  it was unusable. The bar keeps itself up with a `hover` of its own, which is the
  only thing that can.
- **A drag has to show what letting go will do.** Dragging the sidebar divider
  past the snap point narrowed the column to the rail's width while the list
  inside went on drawing two-line rows, because whether it *is* a rail was applied
  on release. `Drag::railed` is the live answer and `drag_sidebar` applies it on
  the frame it crosses.
- **A control sized to its contents resizes when its contents appear.** The
  sidebar's search box grew by half its own height the moment the first character
  was typed, because the clear button is a twenty-eight pixel hit target and the
  search glyph is fourteen — and the whole list under it stepped down and back on
  every empty query. Anything that gains a control conditionally needs a height of
  its own.
- **Zoom must not be laid out.** A zoomed picture is drawn larger than the stage,
  and in the flow that is a flex item asking its parent for room — while the
  parent's measured size is what the zoom is a multiple of. The viewer's picture
  is `absolute` for that reason alone: its size is nothing's business but its own.
- **A trackpad and a mouse wheel arrive as the same event.** `ScrollDelta::Lines`
  is a wheel and `ScrollDelta::Pixels` is two fingers, and in a picture viewer
  they mean opposite things: a wheel means closer, and fingers mean pan, because
  that is what they do in every application on the platform. gpui has `on_pinch`
  for the gesture that really is a zoom.
- **presage reports a download when it has finished downloading.** Upstream's
  `get_attachment` reads the whole stream in one `read_to_end`, which is a
  progress bar that can only slide.
  `get_attachment_reporting` (`vendor/presage`) reads it in chunks and says how
  far it has got; the declared size is the *plaintext* length and what arrives is
  the ciphertext with an IV and a MAC on it, so the fraction is clamped rather
  than allowed to read past one.
- **A one-to-one disappearing timer is kept nowhere a client can read back.** The
  `EXPIRATION_TIMER_UPDATE` message *is* the setting, so a client that does not
  write it down forgets it the moment the window closes —
  `petunia_expire_timers` is that memory, and a group's own record seeds it. When
  the clock starts is Signal's rule and not ours: the moment a message is sent,
  for our own, and the moment it is *read*, for everybody else's, so nothing
  vanishes out of a conversation nobody has opened. The sweep deletes locally and
  only locally: everybody else has the same timer and is running the same clock.
- **The Linux shell had no system libraries in it at all.** macOS hands gpui,
  cpal and the crypto everything they need from frameworks that are simply there,
  so the dev shell needed none of it and the Linux build then found whatever the
  host distribution happened to have, or nothing — most visibly `openssl`, since
  that is the one with a name anybody recognises. `flake.nix` lists them per
  platform now, with `pkg-config` for the `-sys` crates to ask and
  `LD_LIBRARY_PATH` for wayland, the Vulkan loader and the X libraries, which are
  opened by name at run time rather than linked.
- **Encrypting and decrypting are the same read-modify-write.** libsignal loads a
  session record, ratchets it and stores it back, with the store's own awaits in
  the middle — so a delivery receipt going out while the stream opens an envelope
  from that same device is a lost update, and whichever writes second rolls the
  other's work back. The Double Ratchet survives being rolled back, since a stale
  root key still derives the chain the sender is on. The post-quantum ratchet does
  not: its state is sparse and ordered, and every later message from that device
  then fails with `post-quantum ratchet error`, forever. Which is the whole of
  *"message syncing is broken"* — the busiest session an account has is with its
  own phone, which is the one every sync transcript is encrypted to and the one
  everything read there comes back on. `libsignal_service::session_lock` is one
  mutex around the crypto and nothing else — `create_encrypted_messages` and
  `open_envelope` — so the round trips that carry the results still overlap, which
  is what the receipts were taken off the loop for in the first place.
- **A session neither side agrees on does not repair itself.** presage logged the
  failure and skipped the envelope, so a ratchet that had diverged stayed diverged
  and the server redelivered the same unopenable backlog at every reconnect.
  `Received::Undecryptable` names the sender — out of the error for a sealed
  sender message, which does not name one in the clear, and off the envelope for
  anything else — and `reset_session` archives our side and sends a null message,
  which is what makes the other end negotiate a new one. Messages already lost
  stay lost. Once an hour per peer, and remembered across reconnects rather than
  per stream: what prompts a reset is a backlog, and one reset per undecryptable
  message is a message to that peer per message they ever sent.

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
