use crate::domain::model::ThemeName;
use ratatui::style::{Color, Modifier, Style};

// Spacing and responsive layout constants (`tui-design` standards)
pub const PADDING_XS: u16 = 1;
pub const PADDING_SM: u16 = 2;
pub const HEADER_HEIGHT: u16 = 2;
pub const STATUS_BAR_HEIGHT: u16 = 1;
pub const MIN_WIDTH: u16 = 50;
pub const MIN_HEIGHT: u16 = 16;
pub const SPLIT_BREAKPOINT_WIDTH: u16 = 60;
pub const COMPANION_MIN_HEIGHT: u16 = 22;

/// Semantic, restrained multi-theme palette (`indigo`, `blush`, `elite`) with automatic
/// `NO_COLOR` / `TERM=dumb` monochrome degradation.
#[derive(Debug, Clone)]
pub struct Theme {
    pub name: ThemeName,
    pub mono: bool,
    pub base: Style,
    pub muted: Style,
    pub accent: Style,
    pub bold: Style,
    pub success: Style,
    pub warning: Style,
    pub error: Style,
    pub border: Style,
    pub active_border: Style,
    pub highlight: Style,
    pub header: Style,
    pub status_bar: Style,
}

impl Theme {
    pub fn is_mono_env() -> bool {
        let no_color = std::env::var_os("NO_COLOR").is_some();
        let dumb_term = std::env::var("TERM")
            .map(|t| t.eq_ignore_ascii_case("dumb"))
            .unwrap_or(false);
        no_color || dumb_term
    }

    pub fn from_env() -> Self {
        Self::from_env_with_theme(ThemeName::default())
    }

    pub fn from_env_with_theme(theme_name: ThemeName) -> Self {
        Self::with_theme(theme_name, Self::is_mono_env())
    }

    pub fn new(mono: bool) -> Self {
        Self::with_theme(ThemeName::default(), mono)
    }

    pub fn with_theme(theme_name: ThemeName, mono: bool) -> Self {
        if mono {
            return Self {
                name: theme_name,
                mono: true,
                base: Style::default(),
                muted: Style::default().add_modifier(Modifier::DIM),
                accent: Style::default().add_modifier(Modifier::BOLD),
                bold: Style::default().add_modifier(Modifier::BOLD),
                success: Style::default().add_modifier(Modifier::BOLD),
                warning: Style::default().add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                error: Style::default().add_modifier(Modifier::BOLD | Modifier::REVERSED),
                border: Style::default().add_modifier(Modifier::DIM),
                active_border: Style::default().add_modifier(Modifier::BOLD),
                highlight: Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD),
                header: Style::default().add_modifier(Modifier::BOLD),
                status_bar: Style::default().add_modifier(Modifier::REVERSED),
            };
        }

        match theme_name {
            ThemeName::Indigo => {
                let base_fg = Color::Rgb(234, 230, 255); // #EAE6FF Lavender White
                let muted_fg = Color::Rgb(122, 114, 166); // #7A72A6 Slate Violet
                let accent_fg = Color::Rgb(244, 185, 66); // #F4B942 Sunflower Gold
                let active_border_fg = Color::Rgb(124, 106, 242); // #7C6AF2 Arcane Indigo
                let highlight_bg = Color::Rgb(30, 24, 54); // #1E1836 Deep Violet
                let status_bg = Color::Rgb(24, 19, 44);
                Self {
                    name: ThemeName::Indigo,
                    mono: false,
                    base: Style::default().fg(base_fg),
                    muted: Style::default().fg(muted_fg),
                    accent: Style::default().fg(accent_fg).add_modifier(Modifier::BOLD),
                    bold: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    success: Style::default()
                        .fg(Color::Rgb(82, 209, 156))
                        .add_modifier(Modifier::BOLD),
                    warning: Style::default().fg(accent_fg).add_modifier(Modifier::BOLD),
                    error: Style::default()
                        .fg(Color::Rgb(255, 107, 139))
                        .add_modifier(Modifier::BOLD),
                    border: Style::default().fg(muted_fg),
                    active_border: Style::default().fg(active_border_fg),
                    highlight: Style::default()
                        .fg(accent_fg)
                        .bg(highlight_bg)
                        .add_modifier(Modifier::BOLD),
                    header: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    status_bar: Style::default().fg(base_fg).bg(status_bg),
                }
            }
            ThemeName::Blush => {
                let base_fg = Color::Rgb(255, 246, 238); // #FFF6EE Soft Cream
                let muted_fg = Color::Rgb(158, 130, 121); // #9E8279 Rose Taupe
                let accent_fg = Color::Rgb(224, 122, 95); // #E07A5F Terracotta Amber
                let active_border_fg = Color::Rgb(242, 166, 141); // #F2A68D Warm Blush
                let highlight_fg = Color::Rgb(242, 204, 143); // #F2CC8F Honey Gold
                let highlight_bg = Color::Rgb(43, 29, 30); // #2B1D1E Espresso Rose
                let status_bg = Color::Rgb(35, 23, 24);
                Self {
                    name: ThemeName::Blush,
                    mono: false,
                    base: Style::default().fg(base_fg),
                    muted: Style::default().fg(muted_fg),
                    accent: Style::default().fg(accent_fg).add_modifier(Modifier::BOLD),
                    bold: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    success: Style::default()
                        .fg(Color::Rgb(129, 178, 154))
                        .add_modifier(Modifier::BOLD),
                    warning: Style::default()
                        .fg(highlight_fg)
                        .add_modifier(Modifier::BOLD),
                    error: Style::default().fg(accent_fg).add_modifier(Modifier::BOLD),
                    border: Style::default().fg(muted_fg),
                    active_border: Style::default().fg(active_border_fg),
                    highlight: Style::default()
                        .fg(highlight_fg)
                        .bg(highlight_bg)
                        .add_modifier(Modifier::BOLD),
                    header: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    status_bar: Style::default().fg(base_fg).bg(status_bg),
                }
            }
            ThemeName::Elite => {
                let base_fg = Color::Rgb(246, 244, 255); // #F6F4FF Starlight White
                let muted_fg = Color::Rgb(138, 132, 184); // #8A84B8 Muted Lavender
                let accent_fg = Color::Rgb(199, 125, 255); // #C77DFF Electric Orchid
                let active_border_fg = Color::Rgb(123, 104, 238); // #7B68EE Periwinkle
                let highlight_bg = Color::Rgb(25, 21, 46); // #19152E Royal Violet
                let status_bg = Color::Rgb(20, 16, 38);
                Self {
                    name: ThemeName::Elite,
                    mono: false,
                    base: Style::default().fg(base_fg),
                    muted: Style::default().fg(muted_fg),
                    accent: Style::default().fg(accent_fg).add_modifier(Modifier::BOLD),
                    bold: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    success: Style::default()
                        .fg(Color::Rgb(100, 223, 223))
                        .add_modifier(Modifier::BOLD),
                    warning: Style::default()
                        .fg(Color::Rgb(255, 209, 102))
                        .add_modifier(Modifier::BOLD),
                    error: Style::default()
                        .fg(Color::Rgb(255, 93, 143))
                        .add_modifier(Modifier::BOLD),
                    border: Style::default().fg(muted_fg),
                    active_border: Style::default().fg(active_border_fg),
                    highlight: Style::default()
                        .fg(accent_fg)
                        .bg(highlight_bg)
                        .add_modifier(Modifier::BOLD),
                    header: Style::default().fg(base_fg).add_modifier(Modifier::BOLD),
                    status_bar: Style::default().fg(base_fg).bg(status_bg),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn monochrome_theme_strips_ansi_colors() {
        for name in ThemeName::ALL {
            let mono = Theme::with_theme(name, true);
            assert!(mono.mono);
            assert_eq!(mono.name, name);
            assert_eq!(mono.base.fg, None);
            assert_eq!(mono.accent.fg, None);
            assert_eq!(mono.success.fg, None);
            assert_eq!(mono.warning.fg, None);
            assert_eq!(mono.error.fg, None);
            assert_eq!(mono.highlight.bg, None);
        }
    }

    #[test]
    fn color_themes_construct_indigo_blush_and_elite_palettes() {
        let indigo = Theme::with_theme(ThemeName::Indigo, false);
        assert!(!indigo.mono);
        assert_eq!(indigo.name, ThemeName::Indigo);
        assert_eq!(indigo.accent.fg, Some(Color::Rgb(244, 185, 66)));
        assert_eq!(indigo.active_border.fg, Some(Color::Rgb(124, 106, 242)));

        let blush = Theme::with_theme(ThemeName::Blush, false);
        assert!(!blush.mono);
        assert_eq!(blush.name, ThemeName::Blush);
        assert_eq!(blush.accent.fg, Some(Color::Rgb(224, 122, 95)));
        assert_eq!(blush.active_border.fg, Some(Color::Rgb(242, 166, 141)));

        let elite = Theme::with_theme(ThemeName::Elite, false);
        assert!(!elite.mono);
        assert_eq!(elite.name, ThemeName::Elite);
        assert_eq!(elite.accent.fg, Some(Color::Rgb(199, 125, 255)));
        assert_eq!(elite.active_border.fg, Some(Color::Rgb(123, 104, 238)));
    }
}
