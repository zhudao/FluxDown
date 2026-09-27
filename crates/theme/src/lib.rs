//! FluxDown GPUI 客户端的主题 schema、Flutter 默认值与运行时装配。
//!
//! 本 crate 是主题单一入口：完整保存 gpui-base 的颜色、圆角、间距、排版和
//! 阴影 token；将 legacy 可表达部分同步给 gpui-component，并把全部 token
//! 投影给应用自有 Base 组件。业务 feature 不在本 crate 中。

mod appearance;
mod definition;
mod extended;
mod manager;

pub use appearance::*;
pub use definition::*;
pub use extended::*;
pub use gpui_base::{
    ColorTokens, RadiusTokens, SemanticThemeTokens, ShadowTokens, SpacingTokens, TextStyleToken,
    TypographyTokens,
};
pub use gpui_component::ThemeMode;
pub use manager::*;

/// 控件高度：全应用唯一一档，页面工具栏、对话框、表单、设置行、列表行内与独立窗口里的
/// 按钮 / 输入框 / 下拉 / 数字输入统一取此值。
///
/// gpui-component 的 `Size::Small` 只有 21px、`Size::Medium` 是 26px（2rem）；统一取 28px，
/// 与活动栏、表头等 chrome 元素的节奏一致。
pub const CONTROL_HEIGHT: gpui::Pixels = gpui::px(28.);
