# Petunia

Petunia is a native Signal client, named after the flower.

- **Signal library:** [`presage`](https://github.com/whisperfish/presage) — follow
  its examples for how to link, send, and receive.
- **GUI framework:** [`gpui`](https://gpui.rs), via the
  [`gpui-ce`](https://github.com/gpui-ce/gpui-ce) community fork, with
  [`gpui-component`](https://github.com/longbridge/gpui-component) for widgets.
  Both are pinned to git revisions in `Cargo.toml`: gpui-component's published
  release does not build against gpui-ce's HEAD.
- **Look and feel:** modeled on [Zed](https://zed.dev) — a collapsible left
  sidebar, one focused conversation, and an optional right details panel. No
  tabs, and no movable pane grid.
- **Themes:** TOML files in `~/.config/petunia/themes/`, documented by
  `themes.example.toml`. A theme carries a full semantic colour token set plus
  typography, and is hot-reloaded. The built-ins are `dark` and `light` —
  neutral cool greys with a violet accent. Not Catppuccin, and not any other
  borrowed scheme.

## Layout

Follow the reference design, not the layout the old iced client used. The iced
version is gone and its arrangement is not a precedent for anything.

- **Sidebar.** Small quiet section headers with their own affordances, and
  two-line entries carrying real metadata — not a flat list of one-line rows.
  Identity sits at the very bottom.
- **Composer.** A rounded card floating over the conversation with its controls
  *inside* it, right-aligned, and a thin context strip beneath. Not a bordered
  field with a row of buttons stacked above or below it.
- **Chrome.** Generous vertical rhythm, hairline borders, rounded chips, muted
  secondary text. Panels are toggled from icons at the window's edges.
- **Messages.** Discord-style runs: an avatar gutter, one header per run,
  hanging indent. This is the one place that deliberately departs from the
  reference, which is not a chat app.

The first version implements only basic features, but the goal is a fully-featured
Signal client. Structure the project so that future features can be added without
reworking what already exists.

## Layout

- `src/signal/` — the Signal engine: the worker thread, the command/event
  protocol, presage's store, petunia's own sqlite tables, and the media cache.
  Framework-agnostic apart from `bridge.rs`.
- `src/data/` — the model the views read: threads, messages, history, the
  sidebar index. Framework-agnostic; no gpui imports belong here.
- `src/config/` — `config.toml`, themes, and keybindings.
- `src/ui/` — everything gpui.

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

## Building

`protoc` must be on `PATH` (a presage dependency generates code from `.proto`
files at build time).
