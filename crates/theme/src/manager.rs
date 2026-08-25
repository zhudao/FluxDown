use std::sync::Arc;

use gpui::{App, Global, Window, linear_color_stop, linear_gradient};
use gpui_component::{Theme as ComponentTheme, ThemeMode};
use serde::{Deserialize, Serialize};

use crate::{FluxThemeDefinition, SemanticThemeTokens};

const TABLE_HOVER_TOP_OPACITY: f32 = 0.78;
const TABLE_HOVER_BOTTOM_OPACITY: f32 = 0.48;

/// 用户主题偏好；`System` 在每次安装时解析当前系统明暗模式。
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThemePreference {
    #[default]
    System,
    Light,
    Dark,
}

impl ThemePreference {
    fn resolve(self, cx: &App) -> ThemeMode {
        match self {
            Self::System => cx.window_appearance().into(),
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
        }
    }
}

/// 当前应用主题的完整快照。
///
/// gpui-component 的 legacy `Theme` 无法持久保存自定义 spacing/shadow；本全局状态
/// 因此保留完整 Base token，并在每次切换后重新投影到 `gpui_base::Theme`。
#[derive(Clone)]
pub struct FluxThemeState {
    definition: Arc<FluxThemeDefinition>,
    preference: ThemePreference,
    mode: ThemeMode,
    tokens: SemanticThemeTokens,
}

impl Global for FluxThemeState {}

impl FluxThemeState {
    /// 当前主题定义。
    pub fn definition(&self) -> &Arc<FluxThemeDefinition> {
        &self.definition
    }

    /// 用户选择的明暗偏好。
    pub fn preference(&self) -> ThemePreference {
        self.preference
    }

    /// 已解析的实际明暗模式。
    pub fn mode(&self) -> ThemeMode {
        self.mode
    }

    /// 当前完整 Base token；应用自有组件应只从这里取值。
    pub fn tokens(&self) -> &SemanticThemeTokens {
        &self.tokens
    }
}

/// 在 `gpui_component::init` 后安装与 Flutter 客户端一致的默认主题。
pub fn init(cx: &mut App) {
    install_theme(
        Arc::new(FluxThemeDefinition::fluxdown_default()),
        ThemePreference::System,
        None,
        cx,
    );
}

/// 返回当前完整主题状态。
///
/// 调用方必须先执行 [`init`] 或 [`install_theme`]；桌面 shell 在创建任何 view 前
/// 建立这一不变量。
pub fn active_theme(cx: &App) -> &FluxThemeState {
    cx.global::<FluxThemeState>()
}

/// 安装新主题定义并把完整 token 同步到 gpui-component 与 gpui-base。
pub fn install_theme(
    definition: Arc<FluxThemeDefinition>,
    preference: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let mode = preference.resolve(cx);
    let tokens = definition.tokens(mode).clone();

    ComponentTheme::change(mode, None, cx);
    {
        let component_theme = ComponentTheme::global_mut(cx);
        component_theme.apply_semantic_tokens(&tokens);
        component_theme.focus_ring = false;
        component_theme.title_bar = tokens.colors.surface;
        component_theme.title_bar_border = tokens.colors.border;
        component_theme.table = tokens.colors.surface;
        component_theme.table_active = tokens.colors.accent;
        component_theme.table_active_border = tokens.colors.primary;
        component_theme.table_even = tokens.colors.surface;
        component_theme.table_head = tokens.colors.muted;
        component_theme.table_head_foreground = tokens.colors.muted_foreground;
        component_theme.table_hover = tokens.colors.muted;
        component_theme.table_row_border = tokens.colors.border;
        component_theme.tokens.table = tokens.colors.surface.into();
        component_theme.tokens.table_active = tokens.colors.accent.into();
        component_theme.tokens.table_even = tokens.colors.surface.into();
        component_theme.tokens.table_head = tokens.colors.muted.into();
        component_theme.tokens.table_hover.background = linear_gradient(
            180.,
            linear_color_stop(tokens.colors.muted.opacity(TABLE_HOVER_TOP_OPACITY), 0.),
            linear_color_stop(tokens.colors.muted.opacity(TABLE_HOVER_BOTTOM_OPACITY), 1.),
        );
    }
    ComponentTheme::sync_base(cx);
    gpui_base::Theme::global_mut(cx).tokens = tokens.clone();
    cx.set_global(FluxThemeState {
        definition,
        preference,
        mode,
        tokens,
    });

    for handle in cx.windows() {
        let _ = handle.update(cx, |_, window, _| window.refresh());
    }
    if let Some(window) = window {
        window.refresh();
    }
}

/// 保留当前主题定义，仅切换明暗偏好。
pub fn set_theme_preference(
    preference: ThemePreference,
    window: Option<&mut Window>,
    cx: &mut App,
) {
    let definition = Arc::clone(active_theme(cx).definition());
    install_theme(definition, preference, window, cx);
}

/// 在亮/暗两种显式模式间切换。
pub fn toggle_theme(window: &mut Window, cx: &mut App) {
    let preference = if active_theme(cx).mode().is_dark() {
        ThemePreference::Light
    } else {
        ThemePreference::Dark
    };
    set_theme_preference(preference, Some(window), cx);
}

/// 系统外观变化时刷新 `System` 偏好；显式亮/暗偏好保持不变。
pub fn sync_system_theme(window: &mut Window, cx: &mut App) {
    if active_theme(cx).preference() == ThemePreference::System {
        set_theme_preference(ThemePreference::System, Some(window), cx);
    }
}
