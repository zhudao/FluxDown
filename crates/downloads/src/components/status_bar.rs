//! P1.6 状态栏：速度 / 任务统计 / 视图描述 / 磁盘剩余 / 限速 / 完成后关机。
//!
//! 限速走 `DownloadsCommand::PatchConfig`（键 `speed_limit_bytes` /
//! `upload_limit_bytes`，单位字节/秒，见 `native/protocol/src/daemon_config.rs`）；
//! 完成后关机走宿主注入的 `ShutdownPort`（状态机与真正执行由 app 侧
//! `power::ShutdownScheduler` 拥有，本文件只发请求 + 每秒刷新倒计时显示）。

use std::{collections::BTreeMap, rc::Rc, time::Duration};

/// 数字输入弹窗的确认回调。
type NumberConfirm = Rc<dyn Fn(i64, &mut App)>;

use fluxdown_protocol::ApplicationErrorCode;
use fluxdown_ui_theme::active_theme;
use gpui::{
    Anchor, App, AppContext as _, ClickEvent, Context, Entity, IntoElement, ParentElement, Render,
    SharedString, Styled, WeakEntity, Window, div, prelude::FluentBuilder as _, px,
};
use gpui_component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    status_bar::StatusBar,
    v_flex,
};

use crate::{
    controller::DownloadsCommand,
    model::{
        TaskState, format_bytes,
        shutdown::ShutdownRequest,
        view_prefs::{ViewDensity, ViewGroupBy, ViewSortKey},
    },
    pages::downloads::DownloadView,
};

/// 下载 / 上传限速预设（MB/s）。
const SPEED_PRESETS_MB: [i64; 4] = [1, 5, 10, 50];
/// 完成后关机延迟预设（分钟）。
const SHUTDOWN_PRESETS_MIN: [i64; 4] = [1, 5, 10, 30];

fn group_by_key(group_by: ViewGroupBy) -> &'static str {
    match group_by {
        ViewGroupBy::None => "viewGroupNone",
        ViewGroupBy::Status => "viewGroupStatus",
        ViewGroupBy::Date => "viewGroupDate",
        ViewGroupBy::Type => "viewGroupType",
        ViewGroupBy::Queue => "viewGroupQueue",
        ViewGroupBy::Site => "viewGroupSite",
        ViewGroupBy::Group => "viewGroupGroup",
    }
}

fn sort_key_key(sort_key: ViewSortKey) -> &'static str {
    match sort_key {
        ViewSortKey::Smart => "viewSortSmart",
        ViewSortKey::Created => "viewSortCreated",
        ViewSortKey::Name => "viewSortName",
        ViewSortKey::Size => "viewSortSize",
        ViewSortKey::Progress => "viewSortProgress",
        ViewSortKey::Speed => "viewSortSpeed",
    }
}

fn format_countdown(remaining: Duration) -> String {
    let total = remaining.as_secs();
    let hours = total / 3600;
    let minutes = (total % 3600) / 60;
    let seconds = total % 60;
    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

/// 预设 MB/s → `speed_limit_bytes`/`upload_limit_bytes` 配置值（字节/秒）。
fn mb_to_bytes(mb: i64) -> i64 {
    mb * 1_048_576
}

/// 自定义弹窗 KB/s 输入 → 配置值（字节/秒）；负值视为 0。
fn kb_to_bytes(kb: i64) -> i64 {
    kb.max(0) * 1024
}

/// 分钟延迟 → `ShutdownRequest::ArmAfter` 所需 `Duration`；负值视为 0。
fn minutes_to_duration(minutes: i64) -> Duration {
    Duration::from_secs(60 * minutes.max(0) as u64)
}

/// 每秒刷新一次倒计时显示；关机被取消（`armed_delay` 变回 `None`）后循环自然退出。
fn spawn_shutdown_ticker(view: WeakEntity<DownloadView>, cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let Ok(still_armed) = view.update(cx, |this, cx| {
                let armed = this
                    .host
                    .shutdown_status
                    .as_ref()
                    .is_some_and(|status| status.get().armed_delay.is_some());
                if armed {
                    cx.notify();
                }
                armed
            }) else {
                break;
            };
            if !still_armed {
                break;
            }
        }
    })
    .detach();
}

fn apply_speed_limit(view: WeakEntity<DownloadView>, key: &'static str, value: i64, cx: &mut App) {
    let _ = view.update(cx, |this, cx| this.execute_config_patch(key, value, cx));
}

/// 单个数字输入确认弹窗：自定义限速（KB/s）与自定义关机延迟（分钟）复用。
struct NumberPromptDialog {
    input: Entity<InputState>,
    confirm_label: SharedString,
    on_confirm: NumberConfirm,
}

impl Render for NumberPromptDialog {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tokens = active_theme(cx).tokens().clone();
        v_flex()
            .gap(tokens.spacing.sm)
            .child(Input::new(&self.input).w_full())
            .child(
                h_flex().w_full().justify_end().child(
                    Button::new("number-prompt-confirm")
                        .primary()
                        .label(self.confirm_label.clone())
                        .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                            let value = this
                                .input
                                .read(cx)
                                .value()
                                .trim()
                                .parse::<i64>()
                                .unwrap_or(0)
                                .max(0);
                            (this.on_confirm)(value, cx);
                            window.close_dialog(cx);
                        })),
                ),
            )
    }
}

#[allow(clippy::too_many_arguments)]
fn open_number_prompt(
    window: &mut Window,
    cx: &mut App,
    title: SharedString,
    placeholder: SharedString,
    confirm_label: SharedString,
    on_confirm: NumberConfirm,
) {
    let view = cx.new(|cx| NumberPromptDialog {
        input: cx.new(|cx| InputState::new(window, cx).placeholder(placeholder)),
        confirm_label,
        on_confirm,
    });
    let input = view.read(cx).input.clone();
    window.open_dialog(cx, move |dialog, _, _| {
        let view = view.clone();
        dialog
            .title(title.clone())
            .w(px(320.))
            .content(move |content, _, _| content.child(view.clone()))
    });
    input.update(cx, |input, cx| input.focus(window, cx));
}

impl DownloadView {
    /// 状态栏冲突 / 失败提示复用现有横幅字段（`last_error`），不新增 `DownloadStrings`。
    fn execute_config_patch(&mut self, key: &'static str, value: i64, cx: &mut Context<Self>) {
        let mut values = BTreeMap::new();
        values.insert(key.to_owned(), value.to_string());
        let expected_revision = self.controller.config_revision();
        let future = self.controller.execute(DownloadsCommand::PatchConfig {
            values,
            expected_revision,
        });
        let conflict_message = SharedString::from(
            self.translator
                .read(cx)
                .text("localServiceConflict")
                .to_owned(),
        );
        let failed_message = self.strings.action_failed.clone();
        cx.spawn(async move |this, cx| {
            if let Err(error) = future.await {
                let message = if error.code == ApplicationErrorCode::Conflict {
                    conflict_message
                } else {
                    failed_message
                };
                let _ = this.update(cx, |this, cx| {
                    this.last_error = Some(message);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn cycle_status_bar_density(&mut self, cx: &mut Context<Self>) {
        self.table_state.update(cx, |table, cx| {
            let delegate = table.delegate_mut();
            let next = match delegate.prefs().density {
                ViewDensity::Comfortable => ViewDensity::Compact,
                ViewDensity::Compact => ViewDensity::Comfortable,
            };
            delegate.prefs_mut().density = next;
            delegate.refresh_view();
            table.refresh(cx);
        });
        self.schedule_persist_prefs(cx);
        cx.notify();
    }

    /// 下载 / 上传限速控件：图标 + 当前值的小按钮，点开自动收起的菜单（预设 + 自定义）。
    /// `config_key` 为 `speed_limit_bytes` 或 `upload_limit_bytes`（字节/秒）。
    fn render_speed_limit_control(
        &self,
        element_id: &'static str,
        icon: IconName,
        title_key: &'static str,
        config_key: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let translator = self.translator.read(cx);
        let value = self
            .controller
            .config_str(config_key)
            .parse::<i64>()
            .unwrap_or(0);
        let off_label = SharedString::from(translator.text("statusSpeedLimitOff").to_owned());
        let trigger_label = if value <= 0 {
            off_label.clone()
        } else {
            SharedString::from(format!("{}/s", format_bytes(value as u64)))
        };
        let title = SharedString::from(translator.text(title_key).to_owned());
        let custom_label = SharedString::from(translator.text("speedLimitCustom").to_owned());
        let unit_hint = SharedString::from(translator.text("statusSpeedLimitKbs").to_owned());
        let confirm_label = SharedString::from(translator.text("confirm").to_owned());
        let view = cx.weak_entity();

        Button::new(element_id)
            .ghost()
            .xsmall()
            .compact()
            .icon(icon)
            .label(trigger_label)
            .tooltip(title.clone())
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                let mut menu = menu.item(
                    PopupMenuItem::new(off_label.clone())
                        .checked(value <= 0)
                        .on_click({
                            let view = view.clone();
                            move |_, _, cx| apply_speed_limit(view.clone(), config_key, 0, cx)
                        }),
                );
                let mut preset_hit = value <= 0;
                for mb in SPEED_PRESETS_MB {
                    let bytes = mb_to_bytes(mb);
                    preset_hit |= bytes == value;
                    menu = menu.item(
                        PopupMenuItem::new(SharedString::from(format!("{mb} MB/s")))
                            .checked(bytes == value)
                            .on_click({
                                let view = view.clone();
                                move |_, _, cx| {
                                    apply_speed_limit(view.clone(), config_key, bytes, cx)
                                }
                            }),
                    );
                }
                menu.separator().item(
                    PopupMenuItem::new(custom_label.clone())
                        .checked(!preset_hit)
                        .on_click({
                            let view = view.clone();
                            let title = title.clone();
                            let unit_hint = unit_hint.clone();
                            let confirm_label = confirm_label.clone();
                            move |_, window, cx| {
                                let view = view.clone();
                                open_number_prompt(
                                    window,
                                    cx,
                                    title.clone(),
                                    unit_hint.clone(),
                                    confirm_label.clone(),
                                    Rc::new(move |kb, cx| {
                                        apply_speed_limit(
                                            view.clone(),
                                            config_key,
                                            kb_to_bytes(kb),
                                            cx,
                                        );
                                    }),
                                );
                            }
                        }),
                )
            })
    }

    /// 完成后关机控件：未启用时是弹出预设菜单的按钮；已启用 / 倒计时中显示状态 + 取消。
    fn render_shutdown_control(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let translator = self.translator.read(cx);
        let Some(shutdown) = self.host.shutdown.clone() else {
            return div().into_any_element();
        };
        let status = self
            .host
            .shutdown_status
            .as_ref()
            .map(|status| status.get())
            .unwrap_or_default();
        let can_arm = self.controller.runtime_stats().active_tasks > 0;
        let view = cx.weak_entity();
        let cancel_label = SharedString::from(translator.text("shutdownCancelButton").to_owned());
        let warning = cx.theme().warning;

        let armed_text = if let Some(remaining) = status.countdown_remaining {
            Some(translator.text_with(
                "shutdownCountdown",
                &[("time", &format_countdown(remaining))],
            ))
        } else {
            status.armed_delay.map(|delay| {
                if delay.is_zero() {
                    translator.text("shutdownArmedHintImmediate").to_owned()
                } else {
                    translator.text_with(
                        "shutdownArmedHint",
                        &[("m", &(delay.as_secs() / 60).to_string())],
                    )
                }
            })
        };
        if let Some(text) = armed_text {
            return Button::new("shutdown-cancel")
                .ghost()
                .xsmall()
                .compact()
                .icon(Icon::new(IconName::Moon).text_color(warning))
                .label(SharedString::from(text))
                .tooltip(cancel_label)
                .on_click(move |_, _, cx| shutdown(ShutdownRequest::Disarm, cx))
                .into_any_element();
        }

        let trigger_label = SharedString::from(translator.text("shutdownTriggerLabel").to_owned());
        let title = SharedString::from(translator.text("shutdownTitle").to_owned());
        let need_active_hint =
            SharedString::from(translator.text("shutdownNeedActiveTask").to_owned());
        let immediate_label = SharedString::from(translator.text("shutdownImmediate").to_owned());
        let custom_label = SharedString::from(translator.text("speedLimitCustom").to_owned());
        let minutes_unit = SharedString::from(translator.text("shutdownMinutesUnit").to_owned());
        let confirm_label = SharedString::from(translator.text("confirm").to_owned());
        let minute_presets: Vec<(i64, SharedString)> = SHUTDOWN_PRESETS_MIN
            .iter()
            .map(|&minutes| {
                let label =
                    translator.text_with("shutdownDelayMinutes", &[("m", &minutes.to_string())]);
                (minutes, SharedString::from(label))
            })
            .collect();

        Button::new("shutdown-trigger")
            .ghost()
            .xsmall()
            .compact()
            .icon(IconName::Moon)
            .label(trigger_label)
            .tooltip(if can_arm {
                title.clone()
            } else {
                need_active_hint
            })
            .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
                let mut menu = menu.item(
                    PopupMenuItem::new(immediate_label.clone())
                        .disabled(!can_arm)
                        .on_click({
                            let view = view.clone();
                            let shutdown = shutdown.clone();
                            move |_, _, cx| {
                                shutdown(ShutdownRequest::ArmAfter(Duration::ZERO), cx);
                                spawn_shutdown_ticker(view.clone(), cx);
                            }
                        }),
                );
                for (minutes, label) in minute_presets.clone() {
                    menu = menu.item(PopupMenuItem::new(label).disabled(!can_arm).on_click({
                        let view = view.clone();
                        let shutdown = shutdown.clone();
                        move |_, _, cx| {
                            shutdown(ShutdownRequest::ArmAfter(minutes_to_duration(minutes)), cx);
                            spawn_shutdown_ticker(view.clone(), cx);
                        }
                    }));
                }
                menu.separator().item(
                    PopupMenuItem::new(custom_label.clone())
                        .disabled(!can_arm)
                        .on_click({
                            let view = view.clone();
                            let shutdown = shutdown.clone();
                            let title = title.clone();
                            let minutes_unit = minutes_unit.clone();
                            let confirm_label = confirm_label.clone();
                            move |_, window, cx| {
                                let view = view.clone();
                                let shutdown = shutdown.clone();
                                open_number_prompt(
                                    window,
                                    cx,
                                    title.clone(),
                                    minutes_unit.clone(),
                                    confirm_label.clone(),
                                    Rc::new(move |minutes, cx| {
                                        shutdown(
                                            ShutdownRequest::ArmAfter(minutes_to_duration(
                                                minutes.max(0),
                                            )),
                                            cx,
                                        );
                                        spawn_shutdown_ticker(view.clone(), cx);
                                    }),
                                );
                            }
                        }),
                )
            })
            .into_any_element()
    }

    /// 视图开关（分组 / 排序 / 密度）：图标 + 当前值的小按钮，点击循环。
    fn render_view_toggle(
        &self,
        id: &'static str,
        icon: IconName,
        label: SharedString,
        tooltip: SharedString,
        on_click: fn(&mut Self, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        Button::new(id)
            .ghost()
            .xsmall()
            .compact()
            .icon(icon)
            .label(label)
            .tooltip(tooltip)
            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| on_click(this, cx)))
    }

    /// 状态栏：左=速度/任务统计，中=分组/排序/密度开关，右=关机/磁盘/限速。
    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let translator = self.translator.read(cx);
        let stats = self.controller.runtime_stats();
        let download_speed = format!("{}/s", format_bytes(stats.total_download_bps.max(0) as u64));
        let upload_speed = format!("{}/s", format_bytes(stats.total_upload_bps.max(0) as u64));
        let disk_free = stats.disk_free_bytes.map(|bytes| {
            translator.text_with("diskSpaceFreeLabel", &[("size", &format_bytes(bytes))])
        });
        let active_tasks = stats.active_tasks;

        let delegate = self.table_state.read(cx).delegate();
        let paused = delegate.count_where(|task| task.state == TaskState::Paused);
        let total = delegate.count_where(|_| true);
        let summary = translator.text_with(
            "statusSummary",
            &[
                ("active", &active_tasks.to_string()),
                ("paused", &paused.to_string()),
                ("total", &total.to_string()),
            ],
        );
        let prefs = delegate.prefs().clone();
        let group_label =
            SharedString::from(translator.text(group_by_key(prefs.group_by)).to_owned());
        let group_tip = SharedString::from(translator.text("viewSectionGroupBy").to_owned());
        let sort_label =
            SharedString::from(translator.text(sort_key_key(prefs.sort_key)).to_owned());
        let sort_tip = SharedString::from(translator.text("viewSectionSort").to_owned());
        let density_key = match prefs.density {
            ViewDensity::Comfortable => "viewDensityComfortable",
            ViewDensity::Compact => "viewDensityCompact",
        };
        let density_label = SharedString::from(translator.text(density_key).to_owned());
        let density_tip = SharedString::from(translator.text("viewSectionDensity").to_owned());

        let tokens = active_theme(cx).tokens().clone();
        let muted = tokens.colors.muted_foreground;
        let download_limit = self.render_speed_limit_control(
            "status-download-limit",
            IconName::ArrowDown,
            "speedLimitTitle",
            "speed_limit_bytes",
            cx,
        );
        let upload_limit = self.render_speed_limit_control(
            "status-upload-limit",
            IconName::ArrowUp,
            "uploadLimit",
            "upload_limit_bytes",
            cx,
        );
        let shutdown_control = self.render_shutdown_control(cx);
        let group_toggle = self.render_view_toggle(
            "status-view-group",
            IconName::LayoutDashboard,
            group_label,
            group_tip,
            |this, cx| this.mutate_prefs(|prefs| prefs.cycle_group_by(), cx),
            cx,
        );
        let sort_toggle = self.render_view_toggle(
            "status-view-sort",
            match prefs.sort_dir {
                crate::model::view_prefs::SortDir::Asc => IconName::SortAscending,
                crate::model::view_prefs::SortDir::Desc => IconName::SortDescending,
            },
            sort_label,
            sort_tip,
            |this, cx| this.mutate_prefs(|prefs| prefs.cycle_sort(), cx),
            cx,
        );
        let density_toggle = self.render_view_toggle(
            "status-view-density",
            IconName::ALargeSmall,
            density_label,
            density_tip,
            |this, cx| this.cycle_status_bar_density(cx),
            cx,
        );
        let speed_cell = |icon: IconName, text: String| {
            h_flex()
                .gap_1()
                .items_center()
                .px_2()
                .child(Icon::new(icon).size(px(12.)).text_color(muted))
                .child(text)
        };

        StatusBar::new()
            .border_t_1()
            .border_color(tokens.colors.border)
            .text_xs()
            .left(speed_cell(IconName::ArrowDown, download_speed))
            .left(speed_cell(IconName::ArrowUp, upload_speed))
            .left(div().px_2().text_color(muted).child(summary))
            .right(div().px_1().child(download_limit))
            .right(div().px_1().child(upload_limit))
            .when_some(disk_free, |this, disk_free| {
                this.right(div().px_2().text_color(muted).child(disk_free))
            })
            .right(div().px_1().child(shutdown_control))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(group_toggle)
                    .child(sort_toggle)
                    .child(density_toggle),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{format_countdown, kb_to_bytes, mb_to_bytes, minutes_to_duration};

    #[test]
    fn mb_preset_converts_to_bytes_per_second() {
        assert_eq!(mb_to_bytes(1), 1_048_576);
        assert_eq!(mb_to_bytes(50), 52_428_800);
    }

    #[test]
    fn kb_custom_input_converts_to_bytes_and_clamps_negative() {
        assert_eq!(kb_to_bytes(512), 524_288);
        assert_eq!(kb_to_bytes(0), 0);
        assert_eq!(kb_to_bytes(-10), 0);
    }

    #[test]
    fn minutes_preset_converts_to_duration_and_clamps_negative() {
        assert_eq!(minutes_to_duration(5), Duration::from_secs(300));
        assert_eq!(minutes_to_duration(0), Duration::ZERO);
        assert_eq!(minutes_to_duration(-1), Duration::ZERO);
    }

    #[test]
    fn countdown_switches_from_mm_ss_to_h_mm_ss_at_one_hour() {
        assert_eq!(format_countdown(Duration::from_secs(59)), "0:59");
        assert_eq!(format_countdown(Duration::from_secs(3599)), "59:59");
        assert_eq!(format_countdown(Duration::from_secs(3600)), "1:00:00");
        assert_eq!(format_countdown(Duration::from_secs(3661)), "1:01:01");
    }
}
