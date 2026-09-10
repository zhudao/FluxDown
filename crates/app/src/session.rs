//! 单一 agent 会话实体：所有快照 / 事件的唯一入口，向任意数量窗口的任意能力视图广播。
//!
//! 视图通过 [`attach`] 订阅：先用最近全量快照秒开首帧，再用 `system.snapshot`
//! 重新对齐游标并回放缓冲事件；订阅随视图销毁自动解除，主窗口关闭不影响其他窗口。

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

use fluxdown_protocol::{
    AgentSnapshot, EventFrame, RpcErrorData, ServiceEvent, Snapshot, SnapshotBody,
};
use gpui::{App, Context, Entity, EventEmitter};

use crate::agent_client::{AgentClient, AgentClientEvent};

/// 会话向订阅者广播的信号。
pub enum SessionSignal {
    /// 连接 / 重连后的全量快照。
    Snapshot(Arc<Snapshot>),
    /// 单个事件帧。
    Event(Arc<EventFrame>),
    /// 连接断开，等待重连。
    Stale,
    /// 不可恢复错误（协议不兼容 / 未授权）。
    Fatal(RpcErrorData),
}

/// agent 会话状态：最近一次全量快照与连接健康度。
pub struct AgentSession {
    client: Arc<AgentClient>,
    latest: Option<Arc<Snapshot>>,
    stale: bool,
}

impl EventEmitter<SessionSignal> for AgentSession {}

impl AgentSession {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self {
            client,
            latest: None,
            stale: true,
        }
    }

    /// 最近一次全量快照（不折叠事件）。
    #[must_use]
    pub fn latest(&self) -> Option<&Arc<Snapshot>> {
        self.latest.as_ref()
    }

    /// 最近快照的 agent 主体。
    #[must_use]
    pub fn agent_snapshot(&self) -> Option<&AgentSnapshot> {
        self.latest.as_deref().and_then(agent_body)
    }

    /// `attach` 重对齐拿到的更新快照回填 `latest`（同 epoch 且序号不倒退），
    /// 让后续窗口的首帧更接近当前状态。
    pub fn absorb_snapshot(&mut self, snapshot: &Arc<Snapshot>) {
        let fresher = self.latest.as_ref().is_none_or(|latest| {
            latest.epoch != snapshot.epoch || latest.sequence <= snapshot.sequence
        });
        if fresher {
            self.latest = Some(Arc::clone(snapshot));
        }
    }

    /// 把一批客户端事件逐个转为 [`SessionSignal`] 广播。
    pub fn ingest(&mut self, events: Vec<AgentClientEvent>, cx: &mut Context<Self>) {
        for event in events {
            match event {
                AgentClientEvent::Snapshot(snapshot) => {
                    let snapshot = Arc::new(*snapshot);
                    self.latest = Some(Arc::clone(&snapshot));
                    self.stale = false;
                    cx.emit(SessionSignal::Snapshot(snapshot));
                }
                AgentClientEvent::Event(frame) => {
                    cx.emit(SessionSignal::Event(Arc::new(*frame)));
                }
                AgentClientEvent::Stale => {
                    self.stale = true;
                    cx.emit(SessionSignal::Stale);
                }
                AgentClientEvent::Fatal(error) => {
                    self.stale = true;
                    cx.emit(SessionSignal::Fatal(error));
                }
            }
        }
        cx.notify();
    }
}

/// 快照 / 事件消费者：每个能力视图实现一次，由 [`attach`] 驱动。
pub trait SessionConsumer: Sized + 'static {
    fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>);
    fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>);
    fn mark_stale(&mut self, cx: &mut Context<Self>);
}

/// 快照主体（agent 角色）。
#[must_use]
pub fn agent_body(snapshot: &Snapshot) -> Option<&AgentSnapshot> {
    match &snapshot.body {
        SnapshotBody::Agent(body) => Some(body),
        SnapshotBody::Daemon(_) => None,
    }
}

/// 缓冲帧中位于快照游标之后的部分（同 epoch 且序号更大），保持原顺序。
#[must_use]
pub fn frames_after(buffer: &[Arc<EventFrame>], snapshot: &Snapshot) -> Vec<Arc<EventFrame>> {
    buffer
        .iter()
        .filter(|frame| frame.epoch == snapshot.epoch && frame.sequence > snapshot.sequence)
        .cloned()
        .collect()
}

struct AttachState {
    buffer: RefCell<Vec<Arc<EventFrame>>>,
    primed: Cell<bool>,
}

/// 把视图接到会话上：秒开首帧 → 订阅（未对齐前缓冲）→ 拉快照对齐并回放。
pub fn attach<V: SessionConsumer>(session: &Entity<AgentSession>, view: &Entity<V>, cx: &mut App) {
    let state = Rc::new(AttachState {
        buffer: RefCell::new(Vec::new()),
        primed: Cell::new(false),
    });

    let (latest, stale, client) = {
        let session = session.read(cx);
        (
            session.latest.clone(),
            session.stale,
            Arc::clone(&session.client),
        )
    };
    if let Some(latest) = latest.as_deref().and_then(agent_body) {
        view.update(cx, |view, cx| view.replace_snapshot(latest, cx));
    }
    if stale {
        view.update(cx, |view, cx| view.mark_stale(cx));
    }

    let subscribe_state = Rc::clone(&state);
    view.update(cx, |_, cx| {
        cx.subscribe(session, move |view, _, signal, cx| match signal {
            SessionSignal::Event(frame) => {
                if subscribe_state.primed.get() {
                    view.apply_event(&frame.event, cx);
                } else {
                    subscribe_state.buffer.borrow_mut().push(Arc::clone(frame));
                }
            }
            SessionSignal::Snapshot(snapshot) => {
                subscribe_state.primed.set(true);
                subscribe_state.buffer.borrow_mut().clear();
                if let Some(body) = agent_body(snapshot) {
                    view.replace_snapshot(body, cx);
                }
            }
            SessionSignal::Stale | SessionSignal::Fatal(_) => view.mark_stale(cx),
        })
        .detach();
    });

    let view = view.downgrade();
    let session = session.downgrade();
    let future = client.call_snapshot();
    cx.spawn(async move |cx| {
        let result = future.await.map(Arc::new);
        if let Ok(snapshot) = &result {
            let _ = session.update(cx, |session, _| session.absorb_snapshot(snapshot));
        }
        let _ = view.update(cx, |view, cx| {
            if state.primed.get() {
                return;
            }
            match result {
                Ok(snapshot) => {
                    let replay = frames_after(&state.buffer.borrow(), &snapshot);
                    state.primed.set(true);
                    state.buffer.borrow_mut().clear();
                    if let Some(body) = agent_body(&snapshot) {
                        view.replace_snapshot(body, cx);
                    }
                    for frame in replay {
                        view.apply_event(&frame.event, cx);
                    }
                }
                Err(_) => view.mark_stale(cx),
            }
        });
    })
    .detach();
}

macro_rules! forward_consumer {
    ($($view:ty),* $(,)?) => {
        $(
            impl SessionConsumer for $view {
                fn replace_snapshot(&mut self, snapshot: &AgentSnapshot, cx: &mut Context<Self>) {
                    <$view>::replace_snapshot(self, snapshot, cx);
                }
                fn apply_event(&mut self, event: &ServiceEvent, cx: &mut Context<Self>) {
                    <$view>::apply_event(self, event, cx);
                }
                fn mark_stale(&mut self, cx: &mut Context<Self>) {
                    <$view>::mark_stale(self, cx);
                }
            }
        )*
    };
}

forward_consumer!(
    fluxdown_ui_downloads::DownloadView,
    fluxdown_ui_downloads::QuickCaptureView,
    fluxdown_ui_downloads::QueueManagerView,
    fluxdown_ui_downloads::TaskDetailView,
    fluxdown_ui_downloads::GroupDetailView,
    fluxdown_ui_settings::SettingsStore,
    fluxdown_ui_account::AccountView,
    fluxdown_ui_rss::RssView,
    fluxdown_ui_extensions::ExtensionsView,
);

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use fluxdown_protocol::{DaemonEvent, EventFrame, ServiceEvent, Snapshot, SnapshotBody};

    use super::frames_after;

    fn frame(epoch: &str, sequence: u64) -> Arc<EventFrame> {
        Arc::new(EventFrame {
            epoch: epoch.to_owned(),
            sequence,
            event: ServiceEvent::Daemon(DaemonEvent::TaskDeleted {
                task_id: sequence.to_string(),
            }),
        })
    }

    #[test]
    fn replays_only_frames_after_snapshot_cursor_in_same_epoch() {
        let buffer = vec![
            frame("a", 5),
            frame("a", 6),
            frame("b", 7),
            frame("a", 4),
            frame("a", 8),
        ];
        let snapshot = Snapshot {
            epoch: "a".to_owned(),
            sequence: 5,
            body: SnapshotBody::Agent(Box::default()),
        };
        let replay: Vec<u64> = frames_after(&buffer, &snapshot)
            .iter()
            .map(|frame| frame.sequence)
            .collect();
        assert_eq!(replay, vec![6, 8]);
    }
}
