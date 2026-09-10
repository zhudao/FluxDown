//! eD2K：基础（Kad/UPnP/端口）、服务器（列表 + server.met 订阅）。

use gpui::App;
use gpui_component::IconName;

use super::{SectionContext, subscription};
use crate::ui::{SettingsPage, SettingsSection, SettingsTab};

pub(crate) fn page(ctx: &SectionContext, cx: &mut App) -> SettingsPage {
    SettingsPage::new(
        "ed2k",
        ctx.t("settingsCatEd2k"),
        ctx.t("settingsCatEd2kDesc"),
        IconName::HardDrive,
    )
    .tab(SettingsTab::new("basic", ctx.t("settingsTabGeneral")).section(basic_section(ctx)))
    .tab(SettingsTab::new("servers", ctx.t("settingsTabServers")).section(servers_section(ctx, cx)))
}

fn basic_section(ctx: &SectionContext) -> SettingsSection {
    SettingsSection::new()
        .title(ctx.t("settingsTabGeneral"))
        .row(ctx.item(
            "ed2kEnableKad",
            Some("ed2kEnableKadDesc"),
            ctx.daemon_switch("ed2k_enable_kad"),
        ))
        .row(ctx.item(
            "ed2kEnableUpnp",
            Some("ed2kEnableUpnpDesc"),
            ctx.daemon_switch("ed2k_enable_upnp"),
        ))
        .row(ctx.item(
            "ed2kListenPort",
            Some("ed2kListenPortDesc"),
            ctx.daemon_number("ed2k_listen_port"),
        ))
}

fn servers_section(ctx: &SectionContext, cx: &mut App) -> SettingsSection {
    SettingsSection::new()
        .title(ctx.t("settingsTabServers"))
        .row(subscription::list_item(
            ctx,
            "ed2kServerList",
            "ed2kServerListDesc",
            "ed2kServerPlaceholder",
            "ed2k_server_list",
        ))
        .row(ctx.item(
            "ed2kServerSub",
            Some("ed2kServerSubDesc"),
            ctx.daemon_switch("ed2k_server_sub_enabled"),
        ))
        .row(subscription::list_item(
            ctx,
            "ed2kServerSub",
            "ed2kServerSubDesc",
            "ed2kServerSubPlaceholder",
            "ed2k_server_sub_urls",
        ))
        .row(subscription::status_item(
            ctx,
            subscription::SubscriptionKind::Ed2kServers,
            cx,
        ))
}
