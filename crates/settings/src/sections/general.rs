use fluxdown_ui_components::{ButtonVariant, button, card};
use fluxdown_ui_theme::active_theme;
use gpui::{
    Context, Div, ParentElement, SharedString, Styled, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{Icon, IconName, h_flex, switch::Switch, v_flex};

use crate::view::SettingsView;

struct SettingRow {
    id: &'static str,
    title: SharedString,
    description: SharedString,
    checked: bool,
}

impl SettingsView {
    pub(crate) fn render_general_content(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let startup_rows = [
            SettingRow {
                id: "setting-auto-startup",
                title: self.strings.auto_startup.clone(),
                description: self.strings.auto_startup_desc.clone(),
                checked: false,
            },
            SettingRow {
                id: "setting-close-to-tray",
                title: self.strings.close_to_tray.clone(),
                description: self.strings.close_to_tray_desc.clone(),
                checked: true,
            },
            SettingRow {
                id: "setting-start-minimized",
                title: self.strings.start_minimized.clone(),
                description: self.strings.start_minimized_desc.clone(),
                checked: false,
            },
        ];
        let system_rows = [
            SettingRow {
                id: "setting-floating-ball",
                title: self.strings.floating_ball.clone(),
                description: self.strings.floating_ball_desc.clone(),
                checked: false,
            },
            SettingRow {
                id: "setting-torrent-association",
                title: self.strings.torrent_association.clone(),
                description: self.strings.torrent_association_desc.clone(),
                checked: true,
            },
            SettingRow {
                id: "setting-ed2k-association",
                title: self.strings.ed2k_link_association.clone(),
                description: self.strings.ed2k_link_association_desc.clone(),
                checked: false,
            },
            SettingRow {
                id: "setting-magnet-association",
                title: self.strings.magnet_association.clone(),
                description: self.strings.magnet_association_desc.clone(),
                checked: false,
            },
            SettingRow {
                id: "setting-keep-awake",
                title: self.strings.keep_awake.clone(),
                description: self.strings.keep_awake_desc.clone(),
                checked: false,
            },
            SettingRow {
                id: "setting-analytics",
                title: self.strings.analytics_enabled.clone(),
                description: self.strings.analytics_enabled_desc.clone(),
                checked: true,
            },
        ];
        let titlebar_rows = [
            SettingRow {
                id: "setting-titlebar-pause",
                title: self.strings.show_titlebar_pause.clone(),
                description: self.strings.show_titlebar_pause_desc.clone(),
                checked: true,
            },
            SettingRow {
                id: "setting-titlebar-resume",
                title: self.strings.show_titlebar_resume.clone(),
                description: self.strings.show_titlebar_resume_desc.clone(),
                checked: true,
            },
            SettingRow {
                id: "setting-titlebar-settings",
                title: self.strings.show_titlebar_settings.clone(),
                description: self.strings.show_titlebar_settings_desc.clone(),
                checked: true,
            },
            SettingRow {
                id: "setting-titlebar-theme",
                title: self.strings.show_titlebar_theme.clone(),
                description: self.strings.show_titlebar_theme_desc.clone(),
                checked: true,
            },
        ];

        h_flex()
            .w_full()
            .items_start()
            .gap(tokens.spacing.lg)
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(320.))
                    .gap(tokens.spacing.lg)
                    .child(self.render_setting_group(
                        self.strings.group_startup_tray.clone(),
                        None,
                        startup_rows,
                        cx,
                    ))
                    .child(self.render_setting_group(
                        self.strings.group_system.clone(),
                        None,
                        system_rows,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(320.))
                    .gap(tokens.spacing.lg)
                    .child(self.render_setting_group(
                        self.strings.titlebar_buttons.clone(),
                        Some(self.strings.titlebar_buttons_desc.clone()),
                        titlebar_rows,
                        cx,
                    ))
                    .child(self.render_category_group(cx)),
            )
    }

    fn render_setting_group<const N: usize>(
        &self,
        title: SharedString,
        description: Option<SharedString>,
        rows: [SettingRow; N],
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = active_theme(cx).tokens();
        v_flex()
            .w_full()
            .gap(tokens.spacing.sm)
            .child(
                v_flex()
                    .gap(tokens.spacing.xxs)
                    .child(
                        div()
                            .text_size(tokens.typography.sm.size)
                            .font_weight(tokens.typography.sm.weight)
                            .child(title),
                    )
                    .children(description.map(|description| {
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(description)
                    })),
            )
            .child(
                card(cx).w_full().overflow_hidden().children(
                    rows.into_iter()
                        .enumerate()
                        .map(|(index, row)| self.render_setting_row(row, index + 1 == N, cx)),
                ),
            )
    }

    fn render_setting_row(&self, row: SettingRow, last: bool, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        let (key, inverted) = match row.id {
            "setting-floating-ball" => ("general.floating_ball_enabled".to_owned(), false),
            "setting-torrent-association" => ("torrent_assoc_user_disabled".to_owned(), true),
            "setting-ed2k-association" => ("ed2k_assoc_user_disabled".to_owned(), true),
            "setting-magnet-association" => ("magnet_assoc_user_disabled".to_owned(), true),
            "setting-keep-awake" => ("download.keep_awake".to_owned(), false),
            "setting-titlebar-pause" => ("ui.show_titlebar_pause_all".to_owned(), false),
            "setting-titlebar-resume" => ("ui.show_titlebar_resume_all".to_owned(), false),
            "setting-titlebar-settings" => ("ui.show_titlebar_settings".to_owned(), false),
            "setting-titlebar-theme" => ("ui.show_titlebar_theme".to_owned(), false),
            id => (
                id.strip_prefix("setting-").unwrap_or(id).replace('-', "_"),
                false,
            ),
        };
        let checked = self.controller.bool_value(&key, row.checked) ^ inverted;
        let switch_key = key.clone();
        let set_inverted = inverted;
        h_flex()
            .min_h(px(64.))
            .w_full()
            .items_center()
            .justify_between()
            .gap(tokens.spacing.lg)
            .px(tokens.spacing.md)
            .py(tokens.spacing.sm)
            .when(!last, |this| {
                this.border_b_1().border_color(tokens.colors.border)
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(tokens.spacing.xxs)
                    .child(
                        div()
                            .text_size(tokens.typography.sm.size)
                            .font_weight(tokens.typography.sm.weight)
                            .child(row.title),
                    )
                    .child(
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(row.description),
                    ),
            )
            .child(Switch::new(row.id).checked(checked).on_click(cx.listener(
                move |this, checked: &bool, _, cx| {
                    let future = this
                        .controller
                        .set_bool(switch_key.clone(), *checked ^ set_inverted);
                    cx.spawn(async move |this, cx| {
                        if let Err(error) = future.await {
                            eprintln!("settings update failed: {:?}", error.code);
                        }
                        let _ = this.update(cx, |_, cx| cx.notify());
                    })
                    .detach();
                },
            )))
    }

    fn render_category_group(&self, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens();
        v_flex()
            .w_full()
            .gap(tokens.spacing.sm)
            .child(
                v_flex()
                    .gap(tokens.spacing.xxs)
                    .child(
                        div()
                            .text_size(tokens.typography.sm.size)
                            .font_weight(tokens.typography.sm.weight)
                            .child(self.strings.custom_categories.clone()),
                    )
                    .child(
                        div()
                            .text_size(tokens.typography.xs.size)
                            .text_color(tokens.colors.muted_foreground)
                            .child(self.strings.category_priority_note.clone()),
                    ),
            )
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .gap(tokens.spacing.sm)
                    .child(button(
                        "setting-auto-category-dirs",
                        self.strings.auto_category_dirs.clone(),
                        ButtonVariant::Secondary,
                        cx,
                    ))
                    .child(button(
                        "setting-reset-categories",
                        self.strings.reset_categories.clone(),
                        ButtonVariant::Secondary,
                        cx,
                    ))
                    .child(button(
                        "setting-add-category",
                        self.strings.add_category.clone(),
                        ButtonVariant::Secondary,
                        cx,
                    )),
            )
            .child(
                v_flex()
                    .gap(tokens.spacing.xs)
                    .child(self.render_category_card(
                        self.strings.category_all.clone(),
                        self.strings.builtin_category.clone(),
                        IconName::Folder,
                        cx,
                    ))
                    .child(self.render_category_card(
                        self.strings.category_video.clone(),
                        ".mp4, .mkv, .avi, .mov, .wmv, .webm, .m4v",
                        IconName::GalleryVerticalEnd,
                        cx,
                    ))
                    .child(self.render_category_card(
                        self.strings.category_audio.clone(),
                        ".mp3, .flac, .wav, .aac, .ogg, .m4a, .opus",
                        IconName::File,
                        cx,
                    ))
                    .child(self.render_category_card(
                        self.strings.category_document.clone(),
                        ".pdf, .doc, .docx, .xls, .xlsx, .ppt, .txt",
                        IconName::File,
                        cx,
                    )),
            )
    }

    fn render_category_card(
        &self,
        label: SharedString,
        details: impl Into<SharedString>,
        icon: IconName,
        cx: &mut Context<Self>,
    ) -> Div {
        let tokens = active_theme(cx).tokens();
        let builtin = self.strings.builtin_category.clone();
        card(cx)
            .min_h(px(58.))
            .w_full()
            .px(tokens.spacing.md)
            .py(tokens.spacing.sm)
            .child(
                h_flex()
                    .size_full()
                    .items_center()
                    .gap(tokens.spacing.sm)
                    .child(
                        v_flex()
                            .gap(tokens.spacing.xxs)
                            .text_color(tokens.colors.muted_foreground)
                            .child(Icon::new(IconName::ChevronUp).size(px(10.)))
                            .child(Icon::new(IconName::ChevronDown).size(px(10.))),
                    )
                    .child(
                        Icon::new(icon)
                            .size(px(15.))
                            .text_color(tokens.colors.accent_foreground),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(tokens.spacing.xxs)
                            .child(
                                h_flex()
                                    .gap(tokens.spacing.sm)
                                    .items_center()
                                    .child(
                                        div()
                                            .text_size(tokens.typography.sm.size)
                                            .font_weight(tokens.typography.sm.weight)
                                            .child(label),
                                    )
                                    .child(
                                        div()
                                            .px(tokens.spacing.xs)
                                            .py(px(1.))
                                            .rounded(tokens.radius.sm)
                                            .bg(tokens.colors.accent)
                                            .text_color(tokens.colors.accent_foreground)
                                            .text_size(px(9.))
                                            .child(builtin),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .text_size(tokens.typography.xs.size)
                                    .text_color(tokens.colors.muted_foreground)
                                    .child(details.into()),
                            ),
                    ),
            )
    }
}
