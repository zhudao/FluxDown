//! FluxDown GPUI 客户端的主题 schema、Flutter 默认值与运行时装配。
//!
//! 本 crate 是主题单一入口：完整保存 gpui-base 的颜色、圆角、间距、排版和
//! 阴影 token；将 legacy 可表达部分同步给 gpui-component，并把全部 token
//! 投影给应用自有 Base 组件。业务 feature 不在本 crate 中。

mod appearance;
mod definition;
mod manager;

pub use appearance::*;
pub use definition::*;
pub use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TextStyleToken,
    TypographyTokens,
};
pub use gpui_component::ThemeMode;
pub use manager::*;

/// 表单控件（输入框 / 数字输入 / 下拉按钮 / 行内操作按钮）的统一高度。
///
/// gpui-component 的 `Size::Small` 只有 21px、`Size::Medium` 是 28px 但按钮字号
/// 会跳到 `text_base`；桌面端统一取 28px 高 + `text_sm` 字号，避免同一行里
/// 输入框、下拉与按钮三种高度并存。
pub const CONTROL_HEIGHT: gpui::Pixels = gpui::px(28.);
