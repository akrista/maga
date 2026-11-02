//! Arcane `tachyonfx` shader orchestration for `maga`.
//!
//! Centralizes post-processing shader effects over `ratatui::buffer::Buffer`:
//! - Startup workspace materialization (`WorkspaceIntro`)
//! - Modal entrance transitions (`ModalOpen`)
//! - Server toggle / save / sync feedback pulses (`ServerPulse` & `StatusPulse`)
//! - Inspector selection transition (`InspectorPulse`)
//! - Live theme palette transitions (`ThemeTransition`)
//! - Continuous Maga companion wand/crest shimmer (`CompanionAura`) and header badge shimmer (`HeaderShimmer`)

use crate::theme::Theme;
use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::{Margin, Rect};
use ratatui::style::Color;
use tachyonfx::{
    CellFilter, ColorSpace, Duration as FxDuration, Effect, EffectManager, EffectTimer,
    Interpolation, Motion, RefRect, SimpleRng, fx,
};

/// Unique effect channels managed by [`FxEngine`] so repeated triggers smoothly replace prior animations.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum FxKey {
    #[default]
    WorkspaceIntro,
    ModalOpen,
    ServerPulse,
    InspectorPulse,
    StatusPulse,
    ThemeTransition,
    CompanionAura,
    CompanionPulse,
    HeaderShimmer,
}

/// High-level semantic animation triggers emitted by `App` state transitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxTrigger {
    Startup,
    ModalOpened,
    ServerToggled,
    ServerSaved,
    Synced,
    ConflictAlert,
    ThemeChanged,
    SelectionChanged,
}

/// Shared, dynamically updated layout rectangles captured during `ui::draw`.
///
/// Using [`RefRect`] ensures any in-flight `tachyonfx` effect automatically tracks
/// terminal resizes or layout shifts across frames.
#[derive(Debug, Clone, Default)]
pub struct FxAreas {
    pub screen: RefRect,
    pub header_badge: RefRect,
    pub workspace: RefRect,
    pub server_list: RefRect,
    pub selected_row: RefRect,
    pub detail_panel: RefRect,
    pub companion_portrait: RefRect,
    pub status_bar: RefRect,
    pub active_modal: RefRect,
}

impl FxAreas {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resets transient per-frame areas before widget rendering populates active regions.
    pub fn begin_frame(&self, screen: Rect) {
        self.screen.set(screen);
        self.selected_row.set(Rect::default());
        self.detail_panel.set(Rect::default());
        self.companion_portrait.set(Rect::default());
        self.active_modal.set(Rect::default());
    }
}

/// Manages active `tachyonfx` shader effects and ambient mascot/header auras.
#[derive(Debug, Default)]
pub struct FxEngine {
    pub areas: FxAreas,
    manager: EffectManager<FxKey>,
    ambient_active: bool,
    transient_remaining_ms: u32,
}

impl FxEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if any effect (transient or ambient) is currently registered.
    pub fn is_running(&self) -> bool {
        self.manager.is_running()
    }

    /// Returns `true` if a high-frame-rate transient transition is actively playing.
    pub fn has_active_transition(&self) -> bool {
        self.transient_remaining_ms > 0
    }

    /// Immediately cancels all active effects and resets ambient state.
    pub fn clear(&mut self) {
        self.manager = EffectManager::default();
        self.ambient_active = false;
        self.transient_remaining_ms = 0;
    }

    /// Registers a concrete `Effect` under a unique [`FxKey`], replacing any running effect with that key.
    pub fn register_unique(&mut self, key: FxKey, effect: Effect, duration_ms: u32) {
        self.manager.add_unique_effect(key, effect);
        if duration_ms > self.transient_remaining_ms {
            self.transient_remaining_ms = duration_ms;
        }
    }

    /// Converts semantic [`FxTrigger`] events into keyed `tachyonfx` effects bound to `self.areas`.
    pub fn apply_triggers(
        &mut self,
        triggers: &[FxTrigger],
        theme: &Theme,
        animations_enabled: bool,
    ) {
        if !animations_enabled || theme.mono {
            self.clear();
            return;
        }

        for trigger in triggers {
            match trigger {
                FxTrigger::Startup => {
                    let fx = build_workspace_intro_effect(self.areas.workspace.clone(), theme);
                    self.register_unique(FxKey::WorkspaceIntro, fx, 360);
                }
                FxTrigger::ModalOpened => {
                    let fx = build_modal_open_effect(self.areas.active_modal.clone(), theme);
                    self.register_unique(FxKey::ModalOpen, fx, 240);
                }
                FxTrigger::ServerToggled => {
                    let row_fx = build_server_pulse_effect(self.areas.selected_row.clone(), theme);
                    self.register_unique(FxKey::ServerPulse, row_fx, 260);
                    let inspector_fx =
                        build_inspector_pulse_effect(self.areas.detail_panel.clone(), theme);
                    self.register_unique(FxKey::InspectorPulse, inspector_fx, 220);
                    let status_fx =
                        build_status_pulse_effect(self.areas.status_bar.clone(), theme, false);
                    self.register_unique(FxKey::StatusPulse, status_fx, 260);
                    let companion_fx = build_companion_pulse_effect(
                        self.areas.companion_portrait.clone(),
                        theme,
                        false,
                    );
                    self.register_unique(FxKey::CompanionPulse, companion_fx, 260);
                }
                FxTrigger::ServerSaved | FxTrigger::Synced => {
                    let inspector_fx =
                        build_inspector_pulse_effect(self.areas.detail_panel.clone(), theme);
                    self.register_unique(FxKey::InspectorPulse, inspector_fx, 260);
                    let status_fx =
                        build_status_pulse_effect(self.areas.status_bar.clone(), theme, false);
                    self.register_unique(FxKey::StatusPulse, status_fx, 280);
                    let companion_fx = build_companion_pulse_effect(
                        self.areas.companion_portrait.clone(),
                        theme,
                        false,
                    );
                    self.register_unique(FxKey::CompanionPulse, companion_fx, 260);
                }
                FxTrigger::ConflictAlert => {
                    let status_fx =
                        build_status_pulse_effect(self.areas.status_bar.clone(), theme, true);
                    self.register_unique(FxKey::StatusPulse, status_fx, 320);
                    let companion_fx = build_companion_pulse_effect(
                        self.areas.companion_portrait.clone(),
                        theme,
                        true,
                    );
                    self.register_unique(FxKey::CompanionPulse, companion_fx, 300);
                    if self.areas.active_modal.get().width > 0 {
                        let modal_fx =
                            build_modal_open_effect(self.areas.active_modal.clone(), theme);
                        self.register_unique(FxKey::ModalOpen, modal_fx, 260);
                    }
                }
                FxTrigger::ThemeChanged => {
                    let screen_fx = build_theme_transition_effect(self.areas.screen.clone(), theme);
                    self.register_unique(FxKey::ThemeTransition, screen_fx, 280);
                    let companion_fx = build_companion_pulse_effect(
                        self.areas.companion_portrait.clone(),
                        theme,
                        false,
                    );
                    self.register_unique(FxKey::CompanionPulse, companion_fx, 280);
                }
                FxTrigger::SelectionChanged => {
                    let row_fx =
                        build_selection_glide_effect(self.areas.selected_row.clone(), theme);
                    self.register_unique(FxKey::ServerPulse, row_fx, 150);
                    let inspector_fx =
                        build_inspector_pulse_effect(self.areas.detail_panel.clone(), theme);
                    self.register_unique(FxKey::InspectorPulse, inspector_fx, 160);
                }
            }
        }
    }

    /// Ensures the continuous Maga companion aura and header shimmer are registered when animations are enabled.
    pub fn ensure_ambient_effects(&mut self, theme: &Theme, animations_enabled: bool) {
        if !animations_enabled || theme.mono {
            if self.ambient_active {
                self.clear();
            }
            return;
        }

        if !self.ambient_active {
            let companion_fx = build_companion_aura_effect(self.areas.companion_portrait.clone());
            self.manager
                .add_unique_effect(FxKey::CompanionAura, companion_fx);

            let header_fx = build_header_shimmer_effect(self.areas.header_badge.clone());
            self.manager
                .add_unique_effect(FxKey::HeaderShimmer, header_fx);

            self.ambient_active = true;
        }
    }

    /// Processes all active effects against `buf` for the given frame delta.
    ///
    /// When `elapsed` is zero (e.g., synchronous unit tests calling `ui::draw`), shaders are not stepped
    /// so initial widget text assertions remain deterministic.
    pub fn process(
        &mut self,
        elapsed: std::time::Duration,
        buf: &mut Buffer,
        theme: &Theme,
        animations_enabled: bool,
    ) {
        if !animations_enabled || theme.mono {
            self.clear();
            return;
        }

        self.ensure_ambient_effects(theme, animations_enabled);

        let ms = elapsed.as_millis().min(u32::MAX as u128) as u32;
        if ms == 0 {
            return;
        }

        self.transient_remaining_ms = self.transient_remaining_ms.saturating_sub(ms);
        let fx_dt = FxDuration::from_millis(ms);
        let full_area = *buf.area();
        self.manager.process_effects(fx_dt, buf, full_area);
    }
}

/// Extracts the primary RGB color from a style's foreground, falling back to Arcane Gold.
fn style_fg_color(theme_style: ratatui::style::Style, fallback: Color) -> Color {
    theme_style.fg.unwrap_or(fallback)
}

/// Extracts the primary RGB color from a style's background, falling back to Deep Violet.
fn style_bg_color(theme_style: ratatui::style::Style, fallback: Color) -> Color {
    theme_style.bg.unwrap_or(fallback)
}

/// Startup workspace materialization (`WorkspaceIntro`):
/// Combines a deterministic `coalesce` reform with a directional arcane `sweep_in` and border glow.
pub fn build_workspace_intro_effect(workspace_area: RefRect, theme: &Theme) -> Effect {
    let deep_bg = style_bg_color(theme.highlight, Color::Rgb(24, 19, 44));
    let border_fg = style_fg_color(theme.active_border, Color::Rgb(124, 106, 242));

    let coalesce = fx::coalesce(EffectTimer::from_ms(280, Interpolation::CubicOut))
        .with_rng(SimpleRng::new(0x4D41_4741));
    let sweep = fx::sweep_in(
        Motion::LeftToRight,
        14,
        4,
        deep_bg,
        EffectTimer::from_ms(340, Interpolation::QuadOut),
    )
    .with_color_space(ColorSpace::Hsl)
    .with_rng(SimpleRng::new(0x4D41_4742));
    let border_glow =
        fx::fade_from_fg(border_fg, EffectTimer::from_ms(360, Interpolation::SineOut))
            .with_filter(CellFilter::Outer(Margin::new(1, 1)));

    fx::dynamic_area(
        workspace_area,
        fx::parallel(&[coalesce, sweep, border_glow]),
    )
}

/// Modal entrance (`ModalOpen`):
/// Materializes modal dialogs with a fast `coalesce` and an accent border/title shimmer.
pub fn build_modal_open_effect(modal_area: RefRect, theme: &Theme) -> Effect {
    let modal_bg = style_bg_color(theme.highlight, Color::Rgb(30, 24, 54));
    let accent_fg = style_fg_color(theme.accent, Color::Rgb(244, 185, 66));

    let coalesce = fx::coalesce(EffectTimer::from_ms(190, Interpolation::QuartOut))
        .with_rng(SimpleRng::new(0x4D4F_444C));
    let sweep = fx::sweep_in(
        Motion::UpToDown,
        8,
        2,
        modal_bg,
        EffectTimer::from_ms(220, Interpolation::CubicOut),
    )
    .with_rng(SimpleRng::new(0x4D4F_444D));
    let border_pulse =
        fx::fade_from_fg(accent_fg, EffectTimer::from_ms(240, Interpolation::QuadOut))
            .with_filter(CellFilter::Outer(Margin::new(1, 1)));

    fx::dynamic_area(modal_area, fx::parallel(&[coalesce, sweep, border_pulse]))
}

/// Server toggle pulse (`ServerPulse`):
/// Sweeps a golden/theme accent wave across the toggled server row.
pub fn build_server_pulse_effect(row_area: RefRect, theme: &Theme) -> Effect {
    let accent_fg = style_fg_color(theme.accent, Color::Rgb(244, 185, 66));
    let highlight_bg = style_bg_color(theme.highlight, Color::Rgb(30, 24, 54));

    let fg_flash = fx::fade_from_fg(
        accent_fg,
        EffectTimer::from_ms(240, Interpolation::CubicOut),
    );
    let sweep = fx::sweep_in(
        Motion::LeftToRight,
        10,
        0,
        highlight_bg,
        EffectTimer::from_ms(260, Interpolation::QuadOut),
    );

    fx::dynamic_area(row_area, fx::parallel(&[fg_flash, sweep]))
}

/// Subtle selection glide (`ServerPulse` on `j`/`k` navigation):
/// Smoothly interpolates the newly highlighted row foreground from the active border tone.
pub fn build_selection_glide_effect(row_area: RefRect, theme: &Theme) -> Effect {
    let border_fg = style_fg_color(theme.active_border, Color::Rgb(124, 106, 242));
    let glide = fx::fade_from_fg(border_fg, EffectTimer::from_ms(150, Interpolation::QuadOut));
    fx::dynamic_area(row_area, glide)
}

/// Status bar notification pulse (`StatusPulse`):
/// Flashes the status bar from the theme's accent or warning/error color when a sync or conflict occurs.
pub fn build_status_pulse_effect(status_area: RefRect, theme: &Theme, is_alert: bool) -> Effect {
    let pulse_color = if is_alert {
        style_fg_color(theme.warning, Color::Rgb(244, 185, 66))
    } else {
        style_fg_color(theme.success, Color::Rgb(82, 209, 156))
    };

    let fade = fx::fade_from_fg(
        pulse_color,
        EffectTimer::from_ms(280, Interpolation::CubicOut),
    );
    fx::dynamic_area(status_area, fade)
}

/// Inspector panel transition (`InspectorPulse`):
/// Smoothly fades the inner text of the `DETAILS` panel when switching or updating a server.
pub fn build_inspector_pulse_effect(detail_area: RefRect, theme: &Theme) -> Effect {
    let muted_fg = style_fg_color(theme.muted, Color::Rgb(122, 114, 166));
    let inner_fade = fx::fade_from_fg(muted_fg, EffectTimer::from_ms(160, Interpolation::QuadOut))
        .with_filter(CellFilter::Inner(Margin::new(1, 1)));

    fx::dynamic_area(detail_area, inner_fade)
}

/// Live theme palette transition (`ThemeTransition`):
/// Smoothly shifts HSL and interpolates foregrounds across the screen when cycling `indigo`, `blush`, and `elite`.
pub fn build_theme_transition_effect(screen_area: RefRect, theme: &Theme) -> Effect {
    let border_fg = style_fg_color(theme.active_border, Color::Rgb(124, 106, 242));
    let hsl_wave = fx::ping_pong(fx::hsl_shift_fg(
        [18.0, 10.0, 8.0],
        EffectTimer::from_ms(140, Interpolation::SineInOut),
    ));
    let fg_blend = fx::fade_from_fg(
        border_fg,
        EffectTimer::from_ms(260, Interpolation::CubicOut),
    )
    .with_color_space(ColorSpace::Hsl);

    fx::dynamic_area(screen_area, fx::parallel(&[hsl_wave, fg_blend]))
}

/// Predicate matching non-empty Unicode Braille sub-cell glyphs (`⠁..⣿`), arcane ASCII density glyphs,
/// Chibi Kaomoji strokes, and arcane/gold cells in Maga's Hero Showcase, Background, and Companion panels.
fn is_maga_arcane_or_gold_cell(cell: &Cell) -> bool {
    let sym = cell.symbol();
    let is_braille = sym
        .chars()
        .next()
        .is_some_and(|ch| ('\u{2801}'..='\u{28FF}').contains(&ch));
    let is_density_or_chibi_glyph = is_braille
        || matches!(
            sym,
            ":" | "-"
                | "="
                | "+"
                | "*"
                | "#"
                | "%"
                | "@"
                | "▀"
                | "/"
                | "\\"
                | "▲"
                | "🌻"
                | "🔮"
                | "✧"
                | "✦"
                | "⋆"
                | "⚡"
        );
    let matches_color = |c: Color| match c {
        Color::Rgb(r, g, b) => {
            let is_gold = r >= 180 && g >= 110;
            let is_arcane = b >= 110 || r >= 180;
            is_gold || is_arcane
        }
        _ => false,
    };
    is_density_or_chibi_glyph && (matches_color(cell.fg) || matches_color(cell.bg))
}

/// Continuous Maga companion wand & crest aura (`CompanionAura`):
/// Gently breathes warm hue and lightness across the arcane ASCII density glyphs in Maga's portrait.
pub fn build_companion_aura_effect(portrait_area: RefRect) -> Effect {
    let filter = CellFilter::eval_cell(is_maga_arcane_or_gold_cell);
    let shimmer = fx::repeating(fx::ping_pong(
        fx::hsl_shift(
            Some([18.0, 14.0, 11.0]),
            Some([12.0, 8.0, 6.0]),
            EffectTimer::from_ms(1400, Interpolation::SineInOut),
        )
        .with_filter(filter),
    ));
    fx::dynamic_area(portrait_area, shimmer)
}

/// Reactive Maga companion burst (`CompanionPulse`):
/// Plays a fast arcane `coalesce` + accent `fade_from_fg` pulse over the ASCII density portrait
/// when a server is toggled, saved, synced, conflicted, or re-themed.
pub fn build_companion_pulse_effect(
    portrait_area: RefRect,
    theme: &Theme,
    is_alert: bool,
) -> Effect {
    let pulse_fg = if is_alert {
        style_fg_color(theme.warning, Color::Rgb(244, 185, 66))
    } else {
        style_fg_color(theme.accent, Color::Rgb(244, 185, 66))
    };
    let filter = CellFilter::eval_cell(is_maga_arcane_or_gold_cell);
    let coalesce = fx::coalesce(EffectTimer::from_ms(180, Interpolation::QuartOut))
        .with_rng(SimpleRng::new(0x4D41_4743))
        .with_filter(filter.clone());
    let flash = fx::fade_from_fg(pulse_fg, EffectTimer::from_ms(260, Interpolation::CubicOut))
        .with_filter(filter);

    fx::dynamic_area(portrait_area, fx::parallel(&[coalesce, flash]))
}

/// Continuous header brand badge shimmer (`HeaderShimmer`):
/// Subtle golden-violet breathing shift on the `✦ maga` badge in the top-left header.
pub fn build_header_shimmer_effect(header_badge_area: RefRect) -> Effect {
    let shimmer = fx::repeating(fx::ping_pong(fx::hsl_shift_fg(
        [20.0, 10.0, 12.0],
        EffectTimer::from_ms(1600, Interpolation::SineInOut),
    )));
    fx::dynamic_area(header_badge_area, shimmer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::model::ThemeName;
    use ratatui::style::Style;
    use std::time::Duration;

    fn sample_buffer(w: u16, h: u16, fg: Color, bg: Color) -> Buffer {
        let area = Rect::new(0, 0, w, h);
        let mut cell = Cell::new("@");
        cell.set_style(Style::default().fg(fg).bg(bg));
        Buffer::filled(area, cell)
    }

    #[test]
    fn unique_key_replacement_and_clear_on_disabled_or_mono() {
        let mut engine = FxEngine::new();
        let theme = Theme::with_theme(ThemeName::Indigo, false);
        let area = Rect::new(0, 0, 40, 12);
        engine.areas.begin_frame(area);
        engine.areas.workspace.set(area);
        engine.areas.selected_row.set(Rect::new(2, 2, 36, 1));

        // Queue repeated triggers for the same key (`ServerPulse`)
        engine.apply_triggers(
            &[
                FxTrigger::SelectionChanged,
                FxTrigger::ServerToggled,
                FxTrigger::SelectionChanged,
            ],
            &theme,
            true,
        );
        assert!(engine.is_running());
        assert!(engine.has_active_transition());

        // Step once to prune superseded unique effects
        let mut buf = sample_buffer(40, 12, Color::Rgb(234, 230, 255), Color::Rgb(30, 24, 54));
        engine.process(Duration::from_millis(30), &mut buf, &theme, true);
        assert!(engine.is_running());

        // Disabling animations immediately clears all active effects
        engine.process(Duration::from_millis(30), &mut buf, &theme, false);
        assert!(!engine.is_running());
        assert!(!engine.has_active_transition());

        // Monochrome theme also clears and refuses triggers
        let mono_theme = Theme::with_theme(ThemeName::Indigo, true);
        engine.apply_triggers(
            &[FxTrigger::Startup, FxTrigger::ModalOpened],
            &mono_theme,
            true,
        );
        assert!(!engine.is_running());
    }

    #[test]
    fn transient_effects_complete_cleanly_over_simulated_frame_deltas() {
        let mut engine = FxEngine::new();
        let theme = Theme::with_theme(ThemeName::Indigo, false);
        let area = Rect::new(0, 0, 50, 16);
        engine.areas.begin_frame(area);
        engine.areas.workspace.set(area);
        engine.areas.active_modal.set(Rect::new(5, 2, 40, 10));
        engine.areas.selected_row.set(Rect::new(2, 3, 20, 1));
        engine.areas.detail_panel.set(Rect::new(25, 2, 24, 12));
        engine.areas.status_bar.set(Rect::new(0, 15, 50, 1));

        let triggers = [
            FxTrigger::Startup,
            FxTrigger::ModalOpened,
            FxTrigger::ServerToggled,
            FxTrigger::ServerSaved,
            FxTrigger::ConflictAlert,
            FxTrigger::ThemeChanged,
        ];
        engine.apply_triggers(&triggers, &theme, true);
        assert!(engine.has_active_transition());

        let original = sample_buffer(50, 16, Color::Rgb(234, 230, 255), Color::Rgb(30, 24, 54));
        let mut buf = original.clone();

        // Mid-transition frame modifies buffer colors/cells
        engine.process(Duration::from_millis(60), &mut buf, &theme, true);
        assert_ne!(buf, original);
        assert!(engine.has_active_transition());

        // Advance past maximum transient duration (400ms total)
        for _ in 0..8 {
            let mut frame_buf = original.clone();
            engine.process(Duration::from_millis(60), &mut frame_buf, &theme, true);
        }
        assert!(!engine.has_active_transition());
    }

    #[test]
    fn companion_aura_shimmers_gold_pixels_in_color_and_leaves_mono_untouched() {
        let mut engine = FxEngine::new();
        let color_theme = Theme::with_theme(ThemeName::Indigo, false);
        let area = Rect::new(0, 0, 20, 8);
        engine.areas.begin_frame(area);
        engine.areas.companion_portrait.set(area);
        engine.areas.header_badge.set(Rect::new(0, 0, 10, 1));

        // Buffer filled with Maga's sunflower gold (#F4B942)
        let gold = Color::Rgb(244, 185, 66);
        let dark = Color::Rgb(1, 1, 1);
        let base_buf = sample_buffer(20, 8, gold, dark);

        let mut anim_buf = base_buf.clone();
        engine.process(
            Duration::from_millis(350),
            &mut anim_buf,
            &color_theme,
            true,
        );
        // Gold foreground ASCII density glyph (`@`) should be subtly shifted by CompanionAura
        assert_ne!(anim_buf[(5, 4)].fg, gold);

        // Triggering a companion pulse (e.g. ServerToggled) also animates the portrait area
        engine.apply_triggers(&[FxTrigger::ServerToggled], &color_theme, true);
        assert!(engine.has_active_transition());
        let mut pulse_buf = base_buf.clone();
        engine.process(
            Duration::from_millis(60),
            &mut pulse_buf,
            &color_theme,
            true,
        );
        assert_ne!(pulse_buf, base_buf);

        // Monochrome mode must leave buffer 100% untouched
        let mono_theme = Theme::with_theme(ThemeName::Indigo, true);
        let mut mono_buf = sample_buffer(20, 8, Color::Reset, Color::Reset);
        let mono_expected = mono_buf.clone();
        engine.process(Duration::from_millis(350), &mut mono_buf, &mono_theme, true);
        assert_eq!(mono_buf, mono_expected);

        // Animations disabled must also leave buffer 100% untouched
        let mut disabled_buf = base_buf.clone();
        engine.process(
            Duration::from_millis(350),
            &mut disabled_buf,
            &color_theme,
            false,
        );
        assert_eq!(disabled_buf, base_buf);
    }
}
