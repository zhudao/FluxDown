//! 完成后自动关机：需要活跃任务才能 arm，任务清空后倒计时，倒计时中若又出现活跃任务
//! 则取消倒计时回到 armed（不完全解除）；到点执行平台关机命令。
//!
//! 状态机（[`ShutdownState`]）与 GPUI 集成（[`ShutdownScheduler`]）分离：前者是纯逻辑，
//! 便于不依赖 GPUI 单测；后者是承载它的 `Entity`，负责读运行时统计、写共享状态、驱动通知
//! / 托盘 tooltip 与到点执行关机。

use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, Instant};

use fluxdown_ui_downloads::{SharedShutdownStatus, ShutdownPort, ShutdownRequest, ShutdownStatus};
use gpui::{App, AppContext as _, Context, Global};
use gpui_component::WindowExt as _;
use gpui_component::notification::Notification;

use crate::app::Desktop;
use crate::windows::{WindowKey, WindowRegistry};

/// 状态刷新 / 到点检查间隔。
const TICK: Duration = Duration::from_secs(1);

struct ShutdownGlobal {
    status: SharedShutdownStatus,
    port: ShutdownPort,
}

impl Global for ShutdownGlobal {}

/// 安装完成后关机能力：创建承载状态机的 `Entity` 并把只读状态 / 请求端口暴露为全局。
pub fn install(cx: &mut App) {
    if cx.has_global::<ShutdownGlobal>() {
        return;
    }
    let status: SharedShutdownStatus = Rc::new(std::cell::Cell::new(ShutdownStatus::default()));
    let scheduler_status = status.clone();
    let scheduler = cx.new(|cx| ShutdownScheduler::new(scheduler_status, cx));
    let port: ShutdownPort = Rc::new(move |request, cx| {
        scheduler.update(cx, |scheduler, cx| scheduler.request(request, cx));
    });
    cx.set_global(ShutdownGlobal { status, port });
}

/// 状态栏用只读句柄；`install` 尚未调用时返回 `None`。
pub fn status(cx: &App) -> Option<SharedShutdownStatus> {
    cx.try_global::<ShutdownGlobal>()
        .map(|global| global.status.clone())
}

/// 状态栏 / 通知取消按钮用的请求端口；`install` 尚未调用时返回 `None`。
pub fn shutdown_port(cx: &App) -> Option<ShutdownPort> {
    cx.try_global::<ShutdownGlobal>()
        .map(|global| global.port.clone())
}

/// 完成后关机的纯状态机：不依赖 GPUI，`Instant` 由调用方传入，便于单测构造固定时钟。
struct ShutdownState {
    armed_delay: Option<Duration>,
    countdown_until: Option<Instant>,
}

impl ShutdownState {
    fn new() -> Self {
        Self {
            armed_delay: None,
            countdown_until: None,
        }
    }

    #[cfg(test)]
    fn is_armed(&self) -> bool {
        self.armed_delay.is_some()
    }

    #[cfg(test)]
    fn is_counting_down(&self) -> bool {
        self.countdown_until.is_some()
    }

    /// 应用一次外部请求；`ArmAfter` 需要 `active_tasks > 0` 才会被接受，返回是否被接受。
    fn apply(&mut self, request: ShutdownRequest, active_tasks: u32) -> bool {
        match request {
            ShutdownRequest::Disarm => {
                self.armed_delay = None;
                self.countdown_until = None;
                true
            }
            ShutdownRequest::ArmAfter(delay) => {
                if active_tasks == 0 {
                    return false;
                }
                self.armed_delay = Some(delay);
                self.countdown_until = None;
                true
            }
        }
    }

    /// 按当前任务计数与时钟推进一次；返回 `true` 表示本次到点触发关机（armed 状态已复位）。
    fn advance(&mut self, active_tasks: u32, pending_tasks: u32, now: Instant) -> bool {
        let Some(delay) = self.armed_delay else {
            return false;
        };
        let idle = active_tasks == 0 && pending_tasks == 0;
        if idle {
            if self.countdown_until.is_none() {
                self.countdown_until = Some(now + delay);
            }
        } else {
            // 倒计时中又出现活跃 / 等待任务：取消倒计时，回到 armed（不完全解除）。
            self.countdown_until = None;
        }
        let Some(until) = self.countdown_until else {
            return false;
        };
        if now < until {
            return false;
        }
        self.armed_delay = None;
        self.countdown_until = None;
        true
    }

    fn status(&self, now: Instant) -> ShutdownStatus {
        let countdown_remaining = self.countdown_until.map(|until| {
            if until > now {
                until - now
            } else {
                Duration::ZERO
            }
        });
        ShutdownStatus {
            armed_delay: self.armed_delay,
            countdown_remaining,
        }
    }
}

/// 承载 [`ShutdownState`] 的 `Entity`：每秒 tick 一次，折叠 `Desktop` 已跟踪的运行时统计。
pub struct ShutdownScheduler {
    state: ShutdownState,
    status: SharedShutdownStatus,
}

impl ShutdownScheduler {
    fn new(status: SharedShutdownStatus, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |scheduler, cx| {
            loop {
                cx.background_executor().timer(TICK).await;
                if scheduler
                    .update(cx, |scheduler, cx| scheduler.tick(cx))
                    .is_err()
                {
                    break;
                }
            }
        })
        .detach();
        Self {
            state: ShutdownState::new(),
            status,
        }
    }

    fn request(&mut self, request: ShutdownRequest, cx: &mut Context<Self>) {
        let active = Desktop::active_task_count(cx);
        self.state.apply(request, active);
        self.publish(cx);
    }

    fn tick(&mut self, cx: &mut Context<Self>) {
        let (active, pending) = {
            let stats = &Desktop::global(cx).runtime_stats;
            (stats.active_tasks, stats.pending_tasks)
        };
        let fired = self.state.advance(active, pending, Instant::now());
        self.publish(cx);
        if fired {
            spawn_execute_shutdown(cx);
        }
    }

    fn publish(&self, cx: &mut Context<Self>) {
        let status = self.state.status(Instant::now());
        self.status.set(status);
        match status.countdown_remaining {
            Some(remaining) => show_countdown(cx, remaining),
            None => clear_countdown_ui(cx),
        }
    }
}

struct ShutdownNote;

fn show_countdown(cx: &mut App, remaining: Duration) {
    let text = format_remaining(remaining);
    if let Some(handle) = WindowRegistry::handle(cx, &WindowKey::Main) {
        let translator = Desktop::global(cx).translator.read(cx).clone();
        let message = translator.text_with("shutdownCountdown", &[("time", &text)]);
        let cancel_label = translator.text("shutdownCancelButton").to_owned();
        let _ = handle.update(cx, |_, window, cx| {
            let note = Notification::warning(message)
                .id::<ShutdownNote>()
                .autohide(false)
                .action(move |_, _, _| {
                    let cancel_label = cancel_label.clone();
                    gpui_component::button::Button::new("shutdown-cancel-button")
                        .label(cancel_label)
                        .on_click(|_, _, cx| {
                            if let Some(port) = shutdown_port(cx) {
                                port(ShutdownRequest::Disarm, cx);
                            }
                        })
                });
            window.push_notification(note, cx);
        });
        return;
    }
    let translator = Desktop::global(cx).translator.read(cx).clone();
    let tooltip = translator.text_with("shutdownCountdown", &[("time", &text)]);
    crate::tray::set_tooltip(cx, Some(&tooltip));
}

fn clear_countdown_ui(cx: &mut App) {
    if let Some(handle) = WindowRegistry::handle(cx, &WindowKey::Main) {
        let _ = handle.update(cx, |_, window, cx| {
            window.remove_notification::<ShutdownNote>(cx);
        });
    }
    crate::tray::set_tooltip(cx, None);
}

/// `mm:ss`，与 Flutter 端 `ShutdownService.remainingText` 一致。
fn format_remaining(remaining: Duration) -> String {
    let total = remaining.as_secs();
    format!("{:02}:{:02}", total / 60, total % 60)
}

fn spawn_execute_shutdown(cx: &mut App) {
    cx.background_executor()
        .spawn(async { execute_shutdown() })
        .detach();
}

/// 平台关机命令；`FLUXDOWN_SHUTDOWN_DRY_RUN` 设置时只打印，便于 QA 验证不真的关机。
fn execute_shutdown() {
    if std::env::var_os("FLUXDOWN_SHUTDOWN_DRY_RUN").is_some() {
        eprintln!("FluxDown: shutdown dry-run — would power off now");
        return;
    }
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt as _;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = Command::new("shutdown")
            .args(["/s", "/f", "/t", "0"])
            .creation_flags(CREATE_NO_WINDOW)
            .status();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = Command::new("osascript")
            .args(["-e", "tell app \"System Events\" to shut down"])
            .status();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let systemd_ok = Command::new("systemctl")
            .arg("poweroff")
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if !systemd_ok {
            let _ = Command::new("loginctl").arg("poweroff").status();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arm_requires_active_tasks() {
        let mut state = ShutdownState::new();
        assert!(!state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(60)), 0));
        assert!(!state.is_armed());
    }

    #[test]
    fn arm_accepted_with_active_tasks() {
        let mut state = ShutdownState::new();
        assert!(state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(60)), 1));
        assert!(state.is_armed());
    }

    #[test]
    fn active_to_zero_enters_countdown() {
        let mut state = ShutdownState::new();
        state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(60)), 1);
        let now = Instant::now();
        assert!(!state.advance(1, 0, now));
        assert!(!state.is_counting_down());
        assert!(!state.advance(0, 0, now));
        assert!(state.is_counting_down());
    }

    #[test]
    fn countdown_cancelled_by_new_active_task_returns_to_armed() {
        let mut state = ShutdownState::new();
        state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(60)), 1);
        let now = Instant::now();
        state.advance(0, 0, now);
        assert!(state.is_counting_down());
        assert!(!state.advance(1, 0, now));
        assert!(!state.is_counting_down());
        assert!(state.is_armed());
    }

    #[test]
    fn countdown_fires_after_delay_elapses() {
        let mut state = ShutdownState::new();
        state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(5)), 1);
        let now = Instant::now();
        state.advance(0, 0, now);
        assert!(!state.advance(0, 0, now + Duration::from_secs(4)));
        assert!(state.advance(0, 0, now + Duration::from_secs(5)));
        assert!(!state.is_armed());
        assert!(!state.is_counting_down());
    }

    #[test]
    fn disarm_clears_everything() {
        let mut state = ShutdownState::new();
        state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(60)), 1);
        state.advance(0, 0, Instant::now());
        assert!(state.apply(ShutdownRequest::Disarm, 0));
        assert!(!state.is_armed());
        assert!(!state.is_counting_down());
    }

    #[test]
    fn status_reports_remaining_time() {
        let mut state = ShutdownState::new();
        state.apply(ShutdownRequest::ArmAfter(Duration::from_secs(10)), 1);
        let now = Instant::now();
        state.advance(0, 0, now);
        let status = state.status(now);
        assert_eq!(status.armed_delay, Some(Duration::from_secs(10)));
        assert_eq!(status.countdown_remaining, Some(Duration::from_secs(10)));
    }

    #[test]
    fn status_remaining_none_when_not_counting_down() {
        let state = ShutdownState::new();
        let status = state.status(Instant::now());
        assert_eq!(status.armed_delay, None);
        assert_eq!(status.countdown_remaining, None);
    }

    #[test]
    fn format_remaining_pads_minutes_and_seconds() {
        assert_eq!(format_remaining(Duration::from_secs(65)), "01:05");
        assert_eq!(format_remaining(Duration::from_secs(5)), "00:05");
        assert_eq!(format_remaining(Duration::from_secs(600)), "10:00");
    }
}
