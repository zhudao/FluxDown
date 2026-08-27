//! GPUI 设置能力页面与设置分区。
//!
//! 设置通过共享的翻译 Entity 和主题全局状态更新 UI，不依赖其他业务能力。

mod controller;
mod sections;
mod strings;
mod view;

pub use controller::{
    PortFuture, SettingsCommand, SettingsController, SettingsPort, SettingsResult,
};
pub use view::*;

/// 将 FluxDown locale 映射为 gpui-component 支持的 locale。
pub fn component_locale(locale: &str) -> &str {
    if locale == "zh" { "zh-CN" } else { "en" }
}
