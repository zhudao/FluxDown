//! 窗口注册表与进程驻留态。
//!
//! 所有顶层窗口按 [`WindowKey`] 去重；最后一个用户窗口关闭且非 Resident（托盘未安装）
//! 时退出进程。gpui 不会在最后一个窗口关闭时自动退出，这里是唯一的退出判定点。

use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::Arc,
    time::Duration,
};

use gpui::{
    AnyWindowHandle, App, Bounds, Context, Entity, Global, Pixels, Point, Render, Size,
    Subscription, Window, WindowBounds, WindowHandle, WindowId, WindowOptions, point, px, size,
};
use serde_json::{Value, json};

use crate::agent_client::AgentClient;

pub mod group_detail;
pub mod main;
pub mod new_download;
pub mod queue_manager;
pub mod quick_capture;
pub mod selection;
pub mod settings;
pub mod task_detail;

/// 顶层窗口身份：同 key 只允许一个实例。
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WindowKey {
    Main,
    Settings,
    NewDownload,
    QueueManager,
    QuickCapture,
    Selection(String),
    TaskDetail(String),
    GroupDetail(String),
}

impl WindowKey {
    /// 边界持久化偏好键；只有主窗口与设置窗口持久化。
    fn bounds_pref_key(&self) -> Option<&'static str> {
        match self {
            Self::Main => Some("desktop.window.main"),
            Self::Settings => Some("desktop.window.settings"),
            _ => None,
        }
    }
}

pub struct WindowRegistry {
    open: HashMap<WindowKey, AnyWindowHandle>,
    ids: HashMap<WindowId, WindowKey>,
    resident: bool,
    /// 防抖中尚未落盘的窗口边界（退出时强制写一次）。
    pending_bounds: Rc<RefCell<HashMap<&'static str, Value>>>,
    _closed_sub: Subscription,
    _quit_sub: Subscription,
}

impl Global for WindowRegistry {}

impl WindowRegistry {
    /// 安装全局注册表：窗口关闭清理 + 退出判定 + 退出时落盘边界。
    pub fn init(cx: &mut App, client: Arc<AgentClient>) {
        let closed_sub = cx.on_window_closed(|cx, window_id| {
            let should_quit = {
                let registry = cx.global_mut::<Self>();
                if let Some(key) = registry.ids.remove(&window_id) {
                    registry.open.remove(&key);
                }
                registry.should_quit()
            };
            if should_quit {
                cx.quit();
            }
        });
        let pending_bounds = Rc::new(RefCell::new(HashMap::new()));
        let quit_pending = Rc::clone(&pending_bounds);
        let quit_sub = cx.on_app_quit(move |_| {
            let pending: HashMap<&'static str, Value> = quit_pending.borrow_mut().drain().collect();
            let client = Arc::clone(&client);
            async move {
                for (key, value) in pending {
                    let _ = patch_local_preference(&client, key, value).await;
                }
            }
        });
        cx.set_global(Self {
            open: HashMap::new(),
            ids: HashMap::new(),
            resident: false,
            pending_bounds,
            _closed_sub: closed_sub,
            _quit_sub: quit_sub,
        });
    }

    fn should_quit(&self) -> bool {
        self.open.is_empty() && !self.resident
    }

    /// 打开或聚焦窗口。已开 → `activate_window` 并返回 `None`。
    pub fn open_or_focus<V: Render>(
        cx: &mut App,
        key: WindowKey,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> Option<WindowHandle<V>> {
        if let Some(handle) = cx.global::<Self>().open.get(&key).copied() {
            if handle
                .update(cx, |_, window, _| window.activate_window())
                .is_ok()
            {
                return None;
            }
            let registry = cx.global_mut::<Self>();
            registry.open.remove(&key);
            registry.ids.remove(&handle.window_id());
        }
        match cx.open_window(options, build) {
            Ok(handle) => {
                let any: AnyWindowHandle = handle.into();
                let registry = cx.global_mut::<Self>();
                registry.ids.insert(any.window_id(), key.clone());
                registry.open.insert(key, any);
                Some(handle)
            }
            Err(error) => {
                eprintln!("failed to open FluxDown window: {error:#}");
                None
            }
        }
    }

    /// 用新窗口替换同 key 的旧窗口：先注册新窗口再关旧窗口，中间不会出现「无窗口」瞬间
    /// （否则非 Resident 进程会被退出判定误杀）。
    pub fn reopen<V: Render>(
        cx: &mut App,
        key: WindowKey,
        options: WindowOptions,
        build: impl FnOnce(&mut Window, &mut App) -> Entity<V>,
    ) -> Option<WindowHandle<V>> {
        let old = {
            let registry = cx.global_mut::<Self>();
            let old = registry.open.remove(&key);
            if let Some(old) = old {
                registry.ids.remove(&old.window_id());
            }
            old
        };
        let handle = Self::open_or_focus(cx, key, options, build);
        if let Some(old) = old {
            let _ = old.update(cx, |_, window, _| window.remove_window());
        }
        handle
    }

    pub fn close(cx: &mut App, key: &WindowKey) {
        if let Some(handle) = cx.global::<Self>().open.get(key).copied() {
            let _ = handle.update(cx, |_, window, _| window.remove_window());
        }
    }

    #[must_use]
    pub fn is_open(cx: &App, key: &WindowKey) -> bool {
        cx.global::<Self>().open.contains_key(key)
    }

    #[must_use]
    pub fn handle(cx: &App, key: &WindowKey) -> Option<AnyWindowHandle> {
        cx.global::<Self>().open.get(key).copied()
    }

    /// 托盘已安装 → 无窗口也不退出。变为 `false` 且无窗口 → 退出。
    pub fn set_resident(cx: &mut App, resident: bool) {
        let should_quit = {
            let registry = cx.global_mut::<Self>();
            registry.resident = resident;
            registry.should_quit()
        };
        if should_quit {
            cx.quit();
        }
    }

    #[must_use]
    pub fn is_resident(cx: &App) -> bool {
        cx.global::<Self>().resident
    }

    /// 用户窗口数量。
    #[must_use]
    pub fn open_count(cx: &App) -> usize {
        cx.global::<Self>().open.len()
    }

    /// 在根视图上挂窗口边界观察，500ms 防抖后写入设备本地偏好。
    pub fn persist_bounds<V: 'static>(
        key: &WindowKey,
        client: Arc<AgentClient>,
        root: &Entity<V>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let Some(pref_key) = key.bounds_pref_key() else {
            return;
        };
        let pending = Rc::clone(&cx.global::<Self>().pending_bounds);
        let generation = Rc::new(Cell::new(0_u64));
        root.update(cx, |_, cx: &mut Context<V>| {
            cx.observe_window_bounds(window, move |_, window, cx| {
                let value = bounds_to_value(window.window_bounds());
                pending.borrow_mut().insert(pref_key, value.clone());
                let current = generation.get().wrapping_add(1);
                generation.set(current);
                let generation = Rc::clone(&generation);
                let pending = Rc::clone(&pending);
                let client = Arc::clone(&client);
                cx.spawn(async move |_, cx| {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    if generation.get() != current {
                        return;
                    }
                    pending.borrow_mut().remove(pref_key);
                    let _ = patch_local_preference(&client, pref_key, value).await;
                })
                .detach();
            })
            .detach();
        });
    }

    /// 从偏好恢复窗口边界；不可见 / 无记录时居中。
    #[must_use]
    pub fn restore_bounds(
        key: &WindowKey,
        preferences: Option<&Value>,
        default_size: Size<Pixels>,
        cx: &App,
    ) -> WindowBounds {
        let stored = key
            .bounds_pref_key()
            .and(preferences)
            .and_then(value_to_bounds);
        let displays: Vec<Bounds<Pixels>> = cx.displays().iter().map(|d| d.bounds()).collect();
        match stored {
            Some((bounds, maximized)) if bounds_visible(&bounds, &displays) => {
                if maximized {
                    WindowBounds::Maximized(bounds)
                } else {
                    WindowBounds::Windowed(bounds)
                }
            }
            _ => WindowBounds::Windowed(Bounds::centered(None, default_size, cx)),
        }
    }
}

/// `agent.preferences.patch` 设备本地写入（`sync:false`，不进云同步）。
pub async fn patch_local_preference(
    client: &AgentClient,
    key: &str,
    value: Value,
) -> Result<(), fluxdown_protocol::RpcErrorData> {
    client
        .call::<Value, Value>(
            fluxdown_protocol::method::AGENT_PREFERENCES_PATCH,
            Some(json!({ "values": { key: value }, "sync": false })),
        )
        .await
        .map(|_| ())
}

fn bounds_to_value(bounds: WindowBounds) -> Value {
    let (rect, maximized) = match bounds {
        WindowBounds::Windowed(rect) => (rect, false),
        WindowBounds::Maximized(rect) | WindowBounds::Fullscreen(rect) => (rect, true),
    };
    json!({
        "x": f64::from(rect.origin.x),
        "y": f64::from(rect.origin.y),
        "w": f64::from(rect.size.width),
        "h": f64::from(rect.size.height),
        "maximized": maximized,
    })
}

fn value_to_bounds(value: &Value) -> Option<(Bounds<Pixels>, bool)> {
    let num = |key: &str| value.get(key).and_then(Value::as_f64);
    let (x, y, w, h) = (num("x")?, num("y")?, num("w")?, num("h")?);
    let maximized = value
        .get("maximized")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    Some((
        Bounds {
            origin: point(px(x as f32), px(y as f32)),
            size: size(px(w as f32), px(h as f32)),
        },
        maximized,
    ))
}

/// 与 Flutter `window_state_service` 同规则：坐标在 -500..20000、与任一显示器相交且
/// 可见区域 ≥ 100×100。
fn bounds_visible(bounds: &Bounds<Pixels>, displays: &[Bounds<Pixels>]) -> bool {
    let (x, y) = (f32::from(bounds.origin.x), f32::from(bounds.origin.y));
    let (w, h) = (f32::from(bounds.size.width), f32::from(bounds.size.height));
    if !(-500.0..20_000.0).contains(&x)
        || !(-500.0..20_000.0).contains(&y)
        || !(100.0..20_000.0).contains(&w)
        || !(100.0..20_000.0).contains(&h)
    {
        return false;
    }
    displays.iter().any(|display| {
        if !display.intersects(bounds) {
            return false;
        }
        let visible = display.intersect(bounds);
        f32::from(visible.size.width) >= 100.0 && f32::from(visible.size.height) >= 100.0
    })
}

/// 屏幕右下角贴边定位（快速捕获窗口）。
#[must_use]
pub fn bottom_right_bounds(size: Size<Pixels>, margin: Point<Pixels>, cx: &App) -> Bounds<Pixels> {
    let display = cx
        .primary_display()
        .map(|display| display.bounds())
        .unwrap_or_else(|| Bounds {
            origin: point(px(0.), px(0.)),
            size,
        });
    let corner = display.bottom_right();
    Bounds {
        origin: point(
            corner.x - size.width - margin.x,
            corner.y - size.height - margin.y,
        ),
        size,
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, point, px, size};

    use super::{bounds_visible, value_to_bounds};

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<gpui::Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn offscreen_or_barely_visible_bounds_are_rejected() {
        let displays = [rect(0., 0., 1920., 1080.)];
        assert!(bounds_visible(&rect(100., 100., 800., 600.), &displays));
        assert!(!bounds_visible(&rect(1900., 1000., 800., 600.), &displays));
        assert!(!bounds_visible(&rect(-600., 100., 800., 600.), &displays));
        assert!(!bounds_visible(&rect(3000., 100., 800., 600.), &displays));
        assert!(!bounds_visible(&rect(100., 100., 50., 600.), &displays));
    }

    #[test]
    fn stored_bounds_round_trip_maximized_flag() {
        let value =
            serde_json::json!({"x": 10.0, "y": 20.0, "w": 800.0, "h": 600.0, "maximized": true});
        let (bounds, maximized) = value_to_bounds(&value).expect("bounds");
        assert_eq!(bounds, rect(10., 20., 800., 600.));
        assert!(maximized);
        assert!(value_to_bounds(&serde_json::json!({"x": 1})).is_none());
    }
}
