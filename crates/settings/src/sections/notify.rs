//! 通知设置仅包含系统通知；Webhook 使用活动栏独立页面。

use gpui::App;
use gpui_component::IconName;

use super::SectionContext;
use crate::ui::{SettingsPage, SettingsSection};

pub(crate) fn page(ctx: &SectionContext, _cx: &mut App) -> SettingsPage {
    SettingsPage::new(
        "notify",
        ctx.t("settingsCatNotify"),
        ctx.t("notifyGroupSystem"),
        IconName::Bell,
    )
    .sections([SettingsSection::new()
        .title(ctx.t("notifyGroupSystem"))
        .row(ctx.item(
            "notifyOnComplete",
            Some("notifyOnCompleteDesc"),
            ctx.pref_switch("download.notify_on_complete", true),
        ))])
}
