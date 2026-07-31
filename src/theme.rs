use gpui::{App, Global, Hsla, px};
use gpui_component::{Theme as Widgets, ThemeMode};

use crate::config;

/// Petunia's own palette, installed alongside the widget library's so that both
/// read from one theme file.
struct Palette(config::Theme);

impl Global for Palette {}

pub trait ActivePalette {
    fn palette(&self) -> &config::Theme;
}

impl ActivePalette for App {
    fn palette(&self) -> &config::Theme {
        &self.global::<Palette>().0
    }
}

/// Installs a theme, replacing whatever was there. Called once at startup and
/// again on every hot reload.
pub fn install(theme: config::Theme, cx: &mut App) {
    let mode = if theme.is_light() {
        ThemeMode::Light
    } else {
        ThemeMode::Dark
    };

    // Seeds the widget library's own defaults for this mode; everything petunia
    // has an opinion about is overwritten below, and the rest -- charts,
    // tables, skeletons -- keeps a sensible derived value.
    Widgets::change(mode, None, cx);
    apply(&theme, cx);

    cx.set_global(Palette(theme));
    cx.refresh_windows();
}

fn apply(theme: &config::Theme, cx: &mut App) {
    let widgets = Widgets::global_mut(cx);

    widgets.font_family = theme.typography.family.clone().into();
    widgets.font_size = px(theme.typography.ui_size);
    widgets.mono_font_family = theme.typography.mono.clone().into();
    widgets.mono_font_size = px(theme.typography.message_size);

    let colors = &mut widgets.colors;

    colors.background = theme.background;
    colors.foreground = theme.text;
    colors.border = theme.border;
    colors.ring = theme.border_focus;
    colors.caret = theme.accent;
    colors.selection = with_alpha(theme.accent, 0.30);
    colors.overlay = with_alpha(theme.background, 0.60);

    colors.muted = theme.surface;
    colors.muted_foreground = theme.text_muted;

    colors.popover = theme.elevated;
    colors.popover_foreground = theme.text;
    colors.input = theme.sunken;

    colors.accent = theme.hover;
    colors.accent_foreground = theme.text;

    colors.primary = theme.accent;
    colors.primary_hover = with_alpha(theme.accent, 0.90);
    colors.primary_active = with_alpha(theme.accent, 0.80);
    colors.primary_foreground = theme.on_accent;

    colors.secondary = theme.surface;
    colors.secondary_hover = theme.hover;
    colors.secondary_active = theme.active;
    colors.secondary_foreground = theme.text;

    colors.danger = theme.danger;
    colors.danger_foreground = theme.on_accent;
    colors.success = theme.success;
    colors.success_foreground = theme.on_accent;
    colors.warning = theme.warning;
    colors.warning_foreground = theme.on_accent;

    colors.link = theme.accent;
    colors.link_hover = with_alpha(theme.accent, 0.80);
    colors.link_active = theme.accent;

    colors.list = theme.surface;
    colors.list_hover = theme.hover;
    colors.list_active = theme.selected;
    colors.list_active_border = theme.accent;
    colors.list_even = theme.surface;
    colors.list_head = theme.surface;

    colors.sidebar = theme.surface;
    colors.sidebar_foreground = theme.text;
    colors.sidebar_border = theme.border;
    colors.sidebar_accent = theme.hover;
    colors.sidebar_accent_foreground = theme.text;
    colors.sidebar_primary = theme.accent;
    colors.sidebar_primary_foreground = theme.on_accent;

    colors.title_bar = theme.surface;
    colors.title_bar_border = theme.border;
    colors.status_bar = theme.surface;
    colors.status_bar_border = theme.border;

    colors.scrollbar = gpui::transparent_black();
    colors.scrollbar_thumb = with_alpha(theme.text_muted, 0.40);
    colors.scrollbar_thumb_hover = with_alpha(theme.text_muted, 0.70);

    colors.drag_border = theme.accent;
    colors.drop_target = with_alpha(theme.accent, 0.20);
    colors.skeleton = theme.hover;
    colors.window_border = theme.border;
}

fn with_alpha(color: Hsla, alpha: f32) -> Hsla {
    Hsla { a: alpha, ..color }
}
