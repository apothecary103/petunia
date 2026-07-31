#!/usr/bin/env python3
"""Turns Zed's theme families into petunia's token set.

Run against a Zed checkout's `assets/themes` to regenerate `themes/`:

    script/zed-themes.py ~/.cargo/git/checkouts/zed-*/*/assets/themes

The mapping is the interesting part and is documented inline. Zed is an editor
and petunia is not, so a few of its tokens have no counterpart here and a few of
ours have to be derived: petunia's reading column is the deepest surface, which
is Zed's editor background, and its conversation list sits a step above, which is
Zed's panel. Sender colours come from Zed's player palette, which exists for
exactly this — telling people apart at a glance.
"""

import json
import pathlib
import sys

# Zed writes #rrggbbaa; petunia's parser accepts that, but an alpha channel on a
# surface colour would let the window show through itself.
def opaque(colour):
    if colour is None:
        return None
    colour = colour.strip()
    if len(colour) == 9:
        return colour[:7]
    return colour


def pick(style, *keys):
    """The first key that is actually set. Zed leaves some as null per theme."""
    for key in keys:
        value = style.get(key)
        if value:
            return opaque(value)
    return None


def convert(theme):
    style = theme["style"]
    light = theme.get("appearance") == "light"

    players = [
        opaque(player["cursor"])
        for player in style.get("players", [])
        if player.get("cursor")
    ]

    return {
        "name": theme["name"],
        "appearance": "light" if light else "dark",
        # The reading column is the deepest surface, as the editor is in Zed.
        "background": pick(style, "editor.background", "background"),
        # The conversation list sits a step above it, as Zed's panel does.
        "surface": pick(style, "panel.background", "surface.background", "background"),
        "elevated": pick(style, "elevated_surface.background", "surface.background"),
        "sunken": pick(style, "editor.gutter.background", "editor.background"),
        "border": pick(style, "border", "border.variant"),
        "border_focus": pick(style, "border.focused", "border.selected", "border"),
        "text": pick(style, "text"),
        "text_dim": pick(style, "text.muted", "text"),
        "text_muted": pick(style, "text.placeholder", "text.disabled", "text.muted"),
        "hover": pick(style, "element.hover", "ghost_element.hover"),
        "active": pick(style, "element.active", "ghost_element.active"),
        "selected": pick(style, "element.selected", "ghost_element.selected"),
        # The one bright thing: Zed spends its accent on links and cursors.
        "accent": pick(style, "text.accent", "border.focused"),
        "on_accent": pick(style, "editor.background", "background"),
        "success": pick(style, "created", "success"),
        "warning": pick(style, "warning", "modified"),
        "danger": pick(style, "error", "deleted"),
        "accents": players,
        # Zed's syntax palette, carried through whole. A code block in petunia
        # is the same job as a code block in an editor, and inventing a second
        # palette for it would mean two things to keep in step.
        "syntax": {
            name: opaque(entry["color"])
            for name, entry in sorted(style.get("syntax", {}).items())
            if entry.get("color")
        },
    }


def slug(name):
    return name.lower().replace(" ", "-")


TEMPLATE = """# {name}, converted from Zed's theme of the same name.
# Regenerate with script/zed-themes.py; hand edits here will be overwritten.
name = "{name}"
appearance = "{appearance}"

background = "{background}"
surface = "{surface}"
elevated = "{elevated}"
sunken = "{sunken}"
border = "{border}"
border_focus = "{border_focus}"

text = "{text}"
text_dim = "{text_dim}"
text_muted = "{text_muted}"

hover = "{hover}"
active = "{active}"
selected = "{selected}"

accent = "{accent}"
on_accent = "{on_accent}"
success = "{success}"
warning = "{warning}"
danger = "{danger}"

# Sender colours, from Zed's player palette -- which exists for the same job.
accents = [{accents}]

# What a code block is coloured with, straight from Zed's own syntax palette.
[syntax]
{syntax}
"""


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)

    source = pathlib.Path(sys.argv[1])
    out = pathlib.Path(__file__).resolve().parent.parent / "themes"
    out.mkdir(exist_ok=True)

    written = []
    for family in sorted(source.glob("*/*.json")):
        for theme in json.load(open(family))["themes"]:
            fields = convert(theme)
            missing = [key for key, value in fields.items() if value is None]
            if missing:
                print(f"skipping {theme['name']}: no {', '.join(missing)}")
                continue

            fields["accents"] = ", ".join(f'"{colour}"' for colour in fields["accents"])
            fields["syntax"] = "\n".join(
                f'"{name}" = "{colour}"' for name, colour in fields["syntax"].items()
            )
            name = slug(theme["name"])
            (out / f"{name}.toml").write_text(TEMPLATE.format(**fields))
            written.append(name)

    print(f"wrote {len(written)} themes: {', '.join(sorted(written))}")


if __name__ == "__main__":
    main()
