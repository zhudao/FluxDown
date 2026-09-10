//! 通知：系统通知开关 + Webhook 端点与投递记录。

use gpui::App;
use gpui_component::IconName;

use super::{SectionContext, webhook};
use crate::ui::{SettingsPage, SettingsSection};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingsPage {
    SettingsPage::new(
        "notify",
        ctx.t("settingsCatNotify"),
        ctx.t("settingsCatNotifyDesc"),
        IconName::Bell,
    )
    .sections([
        SettingsSection::new()
            .title(ctx.t("notifyGroupSystem"))
            .row(ctx.item(
                "notifyOnComplete",
                Some("notifyOnCompleteDesc"),
                ctx.pref_switch("download.notify_on_complete", true),
            )),
        webhook::endpoints_group(ctx, cx),
        webhook::delivery_log_group(ctx, cx),
    ])
}
