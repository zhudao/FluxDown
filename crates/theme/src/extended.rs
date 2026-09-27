//! FluxDown 自有的扩展语义 token。
//!
//! gpui-base 的 [`SemanticThemeTokens`] 是外部类型，不含桌面列表 UI 需要的状态色、
//! 三级文字色、细分隔线以及标题/注释字号与图标尺寸阶梯。这些值全部从活动 Base
//! token 与明暗模式派生（不进入主题文件 schema），并与 Base token 一起按界面缩放。

use gpui::{FontWeight, Hsla, Pixels, Rgba, px, rgb};
use gpui_component::ThemeMode;

use crate::{SemanticThemeTokens, TextStyleToken, definition::relative_luminance};

/// 主色按钮文字的最低对比度（WCAG AA 正文）。
const MIN_PRIMARY_CONTRAST: f32 = 4.5;

/// 亮色模式下，若主色与其前景对比不足 AA，则逐步压暗主色直至达标。
///
/// 默认蓝 `#3B82F6` 配白字只有约 3.7:1，按钮文字发虚；压暗后（≈`#2563EB`）更沉稳，
/// 暗色模式与用户自选的深色强调色不受影响。`accent_foreground`（强调色文字/图标）
/// 与 `ring` 同步使用调整后的颜色，保证同一强调色在全应用一致。
pub fn ensure_primary_contrast(tokens: &mut SemanticThemeTokens, mode: ThemeMode) {
    if mode.is_dark() {
        return;
    }
    let colors = &mut tokens.colors;
    let foreground = relative_luminance(colors.primary_foreground);
    let mut primary = colors.primary;
    for _ in 0..40 {
        let background = relative_luminance(primary);
        let (light, dark) = if foreground > background {
            (foreground, background)
        } else {
            (background, foreground)
        };
        if (light + 0.05) / (dark + 0.05) >= MIN_PRIMARY_CONTRAST || foreground < background {
            break;
        }
        primary.l = (primary.l - 0.01).max(0.);
    }
    colors.primary = primary;
    colors.accent_foreground = primary;
    colors.ring = primary;
}

/// 扩展颜色：全部是不透明色，叠在选中/悬停底色上不会出现透明度叠加色差。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtendedColors {
    /// 成功（完成）状态色：只用于小面积图标或圆点，不做大面积填充。
    pub success: Hsla,
    /// 警示色：需要注意但不是错误的状态（例如关机倒计时）。
    pub warning: Hsla,
    /// 三级文字：计数、时间、分区标题、占位等最弱信息。
    pub text_tertiary: Hsla,
    /// 细分隔线：比 `border` 更弱，只用于保留下来的少数结构线。
    pub hairline: Hsla,
    /// 列表行 / 导航项悬停底色。
    pub row_hover: Hsla,
    /// 侧栏与顶栏等「退后」区域的底色（比内容区 `surface` 低一个层级）。
    pub chrome: Hsla,
    /// `chrome` 上导航项的悬停底色。
    pub nav_hover: Hsla,
    /// `chrome` 上导航项的选中底色（中性，不用强调色）。
    pub nav_selected: Hsla,
}

/// 图标尺寸阶梯：全应用只用这三档。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconSizes {
    /// 12px：折叠箭头、行内状态点旁的小图标。
    pub sm: Pixels,
    /// 14px：状态栏、按钮内图标、表格内操作。
    pub md: Pixels,
    /// 16px：导航项、文件类型、工具栏主图标。
    pub lg: Pixels,
}

/// 扩展 token 快照（已按界面缩放）。
#[derive(Debug, Clone, PartialEq)]
pub struct ExtendedTokens {
    pub colors: ExtendedColors,
    /// 11/14：计数、分区标题、状态栏。
    pub caption: TextStyleToken,
    /// 15/20 半粗：页面级标题。
    pub title: TextStyleToken,
    pub icon: IconSizes,
}

impl ExtendedTokens {
    /// 从（已缩放的）Base token 与明暗模式派生扩展 token。
    pub fn derive(tokens: &SemanticThemeTokens, mode: ThemeMode, scale: f32) -> Self {
        let colors = &tokens.colors;
        let dark = mode.is_dark();
        let (success, warning) = if dark {
            (rgb(0x30D158), rgb(0xFFB340))
        } else {
            (rgb(0x1F9D55), rgb(0xC77C02))
        };
        let chrome = if dark {
            mix(colors.background, colors.surface, 0.35)
        } else {
            mix(colors.background, colors.muted, 0.35)
        };
        let nav_selected = if dark {
            mix(colors.muted, colors.surface, 0.15)
        } else {
            mix(colors.muted, colors.border, 0.65)
        };
        Self {
            colors: ExtendedColors {
                success: success.into(),
                warning: warning.into(),
                text_tertiary: mix(colors.muted_foreground, colors.surface, 0.38),
                hairline: mix(colors.border, colors.surface, if dark { 0.35 } else { 0.4 }),
                row_hover: mix(colors.muted, colors.surface, if dark { 0.45 } else { 0.4 }),
                chrome,
                nav_hover: mix(chrome, nav_selected, 0.55),
                nav_selected,
            },
            caption: text_style(11. * scale, 14. * scale, FontWeight::NORMAL),
            title: text_style(15. * scale, 20. * scale, FontWeight::SEMIBOLD),
            icon: IconSizes {
                sm: px(12. * scale),
                md: px(14. * scale),
                lg: px(16. * scale),
            },
        }
    }
}

fn text_style(size: f32, line_height: f32, weight: FontWeight) -> TextStyleToken {
    TextStyleToken {
        size: px(size),
        line_height: px(line_height),
        weight,
    }
}

/// 在 sRGB 空间把 `from` 向 `to` 混合 `amount`（0 = from，1 = to），结果不透明。
fn mix(from: Hsla, to: Hsla, amount: f32) -> Hsla {
    let a = from.to_rgb();
    let b = to.to_rgb();
    let t = amount.clamp(0., 1.);
    Hsla::from(Rgba {
        r: a.r + (b.r - a.r) * t,
        g: a.g + (b.g - a.g) * t,
        b: a.b + (b.b - a.b) * t,
        a: 1.,
    })
}

#[cfg(test)]
mod tests {
    use gpui::px;
    use gpui_component::ThemeMode;

    use super::ExtendedTokens;
    use crate::FluxThemeDefinition;

    #[test]
    fn tertiary_text_sits_between_secondary_text_and_surface() {
        let definition = FluxThemeDefinition::fluxdown_default();
        for mode in [ThemeMode::Light, ThemeMode::Dark] {
            let tokens = definition.tokens(mode);
            let extended = ExtendedTokens::derive(tokens, mode, 1.);
            let secondary = tokens.colors.muted_foreground.l;
            let surface = tokens.colors.surface.l;
            let tertiary = extended.colors.text_tertiary.l;
            assert!(
                (secondary.min(surface)..=secondary.max(surface)).contains(&tertiary),
                "{mode:?}: tertiary {tertiary} outside [{secondary}, {surface}]"
            );
            assert!(
                (tertiary - surface).abs() > 0.2,
                "{mode:?}: tertiary too faint"
            );
        }
    }

    #[test]
    fn light_primary_reaches_aa_contrast_and_dark_is_untouched() {
        let definition = FluxThemeDefinition::fluxdown_default();
        let mut light = definition.tokens(ThemeMode::Light).clone();
        let original = light.colors.primary;
        super::ensure_primary_contrast(&mut light, ThemeMode::Light);
        let fg = crate::definition::relative_luminance(light.colors.primary_foreground);
        let bg = crate::definition::relative_luminance(light.colors.primary);
        assert!((fg + 0.05) / (bg + 0.05) >= super::MIN_PRIMARY_CONTRAST);
        assert!(light.colors.primary.l < original.l);
        assert_eq!(light.colors.accent_foreground, light.colors.primary);

        let mut dark = definition.tokens(ThemeMode::Dark).clone();
        let before = dark.clone();
        super::ensure_primary_contrast(&mut dark, ThemeMode::Dark);
        assert_eq!(dark, before);
    }

    #[test]
    fn sizes_follow_ui_scale() {
        let definition = FluxThemeDefinition::fluxdown_default();
        let tokens = definition.tokens(ThemeMode::Light);
        let extended = ExtendedTokens::derive(tokens, ThemeMode::Light, 1.5);
        assert_eq!(extended.caption.size, px(16.5));
        assert_eq!(extended.title.line_height, px(30.));
        assert_eq!(extended.icon.lg, px(24.));
    }
}
