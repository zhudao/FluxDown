use gpui::{Hsla, SharedString, rgb};
use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TypographyTokens,
};
use gpui_component::ThemeMode;
use serde::{Deserialize, Serialize};

/// FluxDown GPUI 主题文件的当前 schema 版本。
pub const THEME_SCHEMA_VERSION: u32 = 1;
const SHADOW_ALPHA: f32 = 46.0 / 255.0;
const LIGHT_SELECTION_ALPHA: f32 = 26.0 / 255.0;
const DARK_SELECTION_ALPHA: f32 = 46.0 / 255.0;

/// 一套同时覆盖亮色与暗色的完整 Base 语义 token。
///
/// 字段保持公开，主题编辑器可直接配置全部 17 个颜色、6 个圆角、7 个间距、
/// 两个字体族、6 个字号/行高/字重角色和 3 档阴影。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FluxThemeDefinition {
    pub schema_version: u32,
    pub name: SharedString,
    pub author: Option<SharedString>,
    pub light: SemanticThemeTokens,
    pub dark: SemanticThemeTokens,
}

impl FluxThemeDefinition {
    /// Flutter 客户端 `defaultLight` / `defaultDark` 使用的 FluxDown 默认主题。
    pub fn fluxdown_default() -> Self {
        Self {
            schema_version: THEME_SCHEMA_VERSION,
            name: "FluxDown Default".into(),
            author: Some("FluxDown".into()),
            light: semantic_tokens(light_colors()),
            dark: semantic_tokens(dark_colors()),
        }
    }

    /// 返回指定明暗模式的完整 token 快照。
    pub fn tokens(&self, mode: ThemeMode) -> &SemanticThemeTokens {
        match mode {
            ThemeMode::Light => &self.light,
            ThemeMode::Dark => &self.dark,
        }
    }

    /// 返回指定明暗模式的完整可变 token 快照。
    pub fn tokens_mut(&mut self, mode: ThemeMode) -> &mut SemanticThemeTokens {
        match mode {
            ThemeMode::Light => &mut self.light,
            ThemeMode::Dark => &mut self.dark,
        }
    }
}

impl Default for FluxThemeDefinition {
    fn default() -> Self {
        Self::fluxdown_default()
    }
}

fn semantic_tokens(colors: ColorTokens) -> SemanticThemeTokens {
    let typography = TypographyTokens {
        sans: "MiSans".into(),
        ..TypographyTokens::default()
    };

    SemanticThemeTokens {
        colors,
        radius: RadiusTokens::default(),
        spacing: SpacingTokens::default(),
        typography,
        shadow: ShadowTokens::elevations(Hsla {
            h: 0.,
            s: 0.,
            l: 0.,
            a: SHADOW_ALPHA,
        }),
    }
}

fn light_colors() -> ColorTokens {
    ColorTokens {
        background: color(0xF8F9FA),
        foreground: color(0x09090B),
        surface: color(0xFFFFFF),
        surface_foreground: color(0x09090B),
        primary: color(0x3B82F6),
        primary_foreground: color(0xFFFFFF),
        secondary: color(0xF1F3F5),
        secondary_foreground: color(0x09090B),
        muted: color(0xF1F3F5),
        muted_foreground: color(0x71717A),
        accent: color_with_alpha(0x3B82F6, LIGHT_SELECTION_ALPHA),
        accent_foreground: color(0x3B82F6),
        destructive: color(0xEF4444),
        destructive_foreground: color(0xFFFFFF),
        border: color(0xE4E4E7),
        input: color(0xE4E4E7),
        ring: color(0x3B82F6),
    }
}

fn dark_colors() -> ColorTokens {
    ColorTokens {
        background: color(0x1C1C1E),
        foreground: color(0xF5F5F7),
        surface: color(0x2C2C2E),
        surface_foreground: color(0xF5F5F7),
        primary: color(0x3B82F6),
        primary_foreground: color(0xFFFFFF),
        secondary: color(0x3A3A3C),
        secondary_foreground: color(0xF5F5F7),
        muted: color(0x3A3A3C),
        muted_foreground: color(0xA1A1A6),
        accent: color_with_alpha(0x3B82F6, DARK_SELECTION_ALPHA),
        accent_foreground: color(0x3B82F6),
        destructive: color(0xEF4444),
        destructive_foreground: color(0xFFFFFF),
        border: color(0x48484A),
        input: color(0x48484A),
        ring: color(0x3B82F6),
    }
}

fn color_with_alpha(value: u32, alpha: f32) -> Hsla {
    Hsla {
        a: alpha,
        ..color(value)
    }
}

fn color(value: u32) -> Hsla {
    Hsla::from(rgb(value))
}

#[cfg(test)]
mod tests {
    use gpui::{Hsla, px, rgb};
    use gpui_base::ColorTokens;
    use gpui_component::ThemeMode;

    use super::FluxThemeDefinition;

    fn color(value: u32) -> Hsla {
        Hsla::from(rgb(value))
    }

    fn color_with_alpha(value: u32, alpha: f32) -> Hsla {
        Hsla {
            a: alpha,
            ..color(value)
        }
    }

    #[test]
    fn default_light_colors_match_flutter_tokens() {
        let theme = FluxThemeDefinition::fluxdown_default();

        assert_eq!(
            theme.light.colors,
            ColorTokens {
                background: color(0xF8F9FA),
                foreground: color(0x09090B),
                surface: color(0xFFFFFF),
                surface_foreground: color(0x09090B),
                primary: color(0x3B82F6),
                primary_foreground: color(0xFFFFFF),
                secondary: color(0xF1F3F5),
                secondary_foreground: color(0x09090B),
                muted: color(0xF1F3F5),
                muted_foreground: color(0x71717A),
                accent: color_with_alpha(0x3B82F6, 26.0 / 255.0),
                accent_foreground: color(0x3B82F6),
                destructive: color(0xEF4444),
                destructive_foreground: color(0xFFFFFF),
                border: color(0xE4E4E7),
                input: color(0xE4E4E7),
                ring: color(0x3B82F6),
            }
        );
        assert_eq!(theme.light.radius.md, px(6.));
        assert_eq!(theme.light.spacing.xxl, px(32.));
        assert_eq!(theme.light.typography.md.size, px(16.));
        assert!(!theme.light.shadow.md.is_empty());
    }

    #[test]
    fn default_dark_colors_match_flutter_tokens() {
        let theme = FluxThemeDefinition::fluxdown_default();

        assert_eq!(
            theme.dark.colors,
            ColorTokens {
                background: color(0x1C1C1E),
                foreground: color(0xF5F5F7),
                surface: color(0x2C2C2E),
                surface_foreground: color(0xF5F5F7),
                primary: color(0x3B82F6),
                primary_foreground: color(0xFFFFFF),
                secondary: color(0x3A3A3C),
                secondary_foreground: color(0xF5F5F7),
                muted: color(0x3A3A3C),
                muted_foreground: color(0xA1A1A6),
                accent: color_with_alpha(0x3B82F6, 46.0 / 255.0),
                accent_foreground: color(0x3B82F6),
                destructive: color(0xEF4444),
                destructive_foreground: color(0xFFFFFF),
                border: color(0x48484A),
                input: color(0x48484A),
                ring: color(0x3B82F6),
            }
        );
    }

    #[test]
    fn full_token_snapshot_round_trips_for_theme_editing() -> Result<(), serde_json::Error> {
        let mut theme = FluxThemeDefinition::fluxdown_default();
        theme.tokens_mut(ThemeMode::Light).spacing.xxl = px(40.);

        let json = serde_json::to_string(&theme)?;
        let restored = serde_json::from_str::<FluxThemeDefinition>(&json)?;

        assert_eq!(restored, theme);
        Ok(())
    }
}
