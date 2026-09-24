//! 单一 agent 会话实体：所有快照 / 事件的唯一入口，向任意数量窗口的任意能力视图广播。
//!
//! 视图通过 [attach] 订阅：从持续折叠的同源快照秒开首帧，后续沿单会话
//! 的严格事件游标更新；订阅随视图销毁自动解除。

use std::sync::Arc;

use fluxdown_protocol::{
    AgentEvent, AgentSnapshot, DaemonEvent, EventFrame, RpcErrorData, ServiceEvent, Snapshot,
    SnapshotBody, accepted_runtime_status, apply_agent_event,
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
    _client: Arc<AgentClient>,
    latest: Option<Arc<Snapshot>>,
    stale: bool,
}

impl EventEmitter<SessionSignal> for AgentSession {}

impl AgentSession {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self {
            _client: client,
            latest: None,
            stale: true,
        }
    }

    /// 最近一次同源完整投影（已折叠有序事件）。
    #[must_use]
    pub fn latest(&self) -> Option<&Arc<Snapshot>> {
        self.latest.as_ref()
    }

    /// 最近快照的 agent 主体。
    #[must_use]
    pub fn agent_snapshot(&self) -> Option<&AgentSnapshot> {
        self.latest.as_deref().and_then(agent_body)
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
                AgentClientEvent::Event(mut frame) => {
                    match self
                        .latest
                        .as_mut()
                        .map(|snapshot| apply_frame(Arc::make_mut(snapshot), &mut frame))
                    {
                        Some(Ok(true)) if !self.stale => {
                            cx.emit(SessionSignal::Event(Arc::new(*frame)));
                        }
                        Some(Ok(_)) => {}
                        _ => {
                            self.mark_stale();
                            cx.emit(SessionSignal::Stale);
                        }
                    }
                }
                AgentClientEvent::Stale => {
                    self.mark_stale();
                    cx.emit(SessionSignal::Stale);
                }
                AgentClientEvent::Fatal(error) => {
                    self.mark_stale();
                    cx.emit(SessionSignal::Fatal(error));
                }
            }
        }
        cx.notify();
    }

    fn mark_stale(&mut self) {
        self.stale = true;
        if let Some(snapshot) = self.latest.as_mut()
            && let SnapshotBody::Agent(body) = &mut Arc::make_mut(snapshot).body
        {
            body.daemon_connected = false;
            body.daemon.task_runtime.clear();
        }
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

/// 仅订阅唯一 AgentSession；同线程同步首帧，没有额外窗口连接或异步快照回放。
pub fn attach<V: SessionConsumer>(session: &Entity<AgentSession>, view: &Entity<V>, cx: &mut App) {
    view.update(cx, |_, cx| {
        cx.subscribe(session, move |view, _, signal, cx| match signal {
            SessionSignal::Event(frame) => view.apply_event(&frame.event, cx),
            SessionSignal::Snapshot(snapshot) => {
                if let Some(body) = agent_body(snapshot) {
                    view.replace_snapshot(body, cx);
                }
            }
            SessionSignal::Stale | SessionSignal::Fatal(_) => view.mark_stale(cx),
        })
        .detach();
    });
    let (latest, stale) = {
        let session = session.read(cx);
        (session.latest.clone(), session.stale)
    };
    if let Some(body) = latest.as_deref().and_then(agent_body) {
        view.update(cx, |view, cx| view.replace_snapshot(body, cx));
    }
    if stale {
        view.update(cx, |view, cx| view.mark_stale(cx));
    }
}

/// 严格连续地把帧并入同一 epoch 的快照；旧帧不可倒灌，缺口不可静默跳过。
fn apply_frame(snapshot: &mut Snapshot, frame: &mut EventFrame) -> Result<bool, ()> {
    if snapshot.epoch != frame.epoch {
        return Err(());
    }
    if frame.sequence <= snapshot.sequence {
        return Ok(false);
    }
    if frame.sequence != snapshot.sequence.saturating_add(1) {
        return Err(());
    }
    let (SnapshotBody::Agent(body), ServiceEvent::Agent(event)) =
        (&mut snapshot.body, &frame.event)
    else {
        return Err(());
    };
    let deliver = match event {
        AgentEvent::Daemon(DaemonEvent::TaskRuntimeChanged(runtime)) => {
            accepted_runtime_status(&body.daemon, runtime).is_some()
        }
        _ => true,
    };
    apply_agent_event(body, event);
    if deliver
        && let ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskRuntimeChanged(runtime))) =
            &mut frame.event
        && let Some(projected) = body.daemon.task_runtime.get(&runtime.task_id)
    {
        runtime.clone_from(projected);
    }
    snapshot.sequence = frame.sequence;
    Ok(deliver)
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
    use fluxdown_protocol::{
        AgentEvent, DaemonEvent, EventFrame, ServiceEvent, Snapshot, SnapshotBody, TaskRuntimeDto,
    };

    use super::apply_frame;

    fn runtime_frame(epoch: &str, sequence: u64, active: u32) -> EventFrame {
        EventFrame {
            epoch: epoch.to_owned(),
            sequence,
            event: ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskRuntimeChanged(
                TaskRuntimeDto {
                    task_id: "task".into(),
                    sample_sequence: sequence,
                    active_transfers: Some(active),
                    ..Default::default()
                },
            ))),
        }
    }

    fn source_snapshot(epoch: &str, sequence: u64) -> Snapshot {
        let task = serde_json::from_value(serde_json::json!({
            "taskId":"task", "url":"https://example.com/x", "fileName":"x", "saveDir":"/tmp",
            "status":1, "downloadedBytes":0, "totalBytes":100, "errorMessage":"",
            "createdAt":"1", "proxyUrl":"", "queueId":"main", "checksum":""
        }))
        .expect("task fixture");
        let mut agent = fluxdown_protocol::AgentSnapshot::default();
        agent.daemon.tasks.push(task);
        Snapshot {
            epoch: epoch.into(),
            sequence,
            body: SnapshotBody::Agent(Box::new(agent)),
        }
    }

    #[test]
    fn late_view_sees_live_runtime_and_old_or_gapped_frames_never_rewind_it() {
        let mut snapshot = source_snapshot("a", 5);
        assert_eq!(
            apply_frame(&mut snapshot, &mut runtime_frame("a", 6, 3)),
            Ok(true)
        );
        assert_eq!(
            apply_frame(&mut snapshot, &mut runtime_frame("a", 6, 0)),
            Ok(false)
        );
        assert_eq!(
            apply_frame(&mut snapshot, &mut runtime_frame("b", 7, 0)),
            Err(())
        );
        assert_eq!(
            apply_frame(&mut snapshot, &mut runtime_frame("a", 8, 0)),
            Err(())
        );
        let SnapshotBody::Agent(ref body) = snapshot.body else {
            panic!("agent snapshot")
        };
        assert_eq!(body.daemon.task_runtime["task"].active_transfers, Some(3));
        assert_eq!(snapshot.sequence, 6);
        let mut older = runtime_frame("a", 7, 9);
        if let ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskRuntimeChanged(runtime))) =
            &mut older.event
        {
            runtime.sample_sequence = 5;
        }
        assert_eq!(apply_frame(&mut snapshot, &mut older), Ok(false));
        assert_eq!(snapshot.sequence, 7);
        let SnapshotBody::Agent(ref body) = snapshot.body else {
            panic!("agent snapshot")
        };
        assert_eq!(body.daemon.task_runtime["task"].active_transfers, Some(3));
        let mut complete = body.daemon.tasks[0].clone();
        complete.status = 3;
        let mut terminal = EventFrame {
            epoch: "a".into(),
            sequence: 8,
            event: ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskChanged(complete))),
        };
        assert_eq!(apply_frame(&mut snapshot, &mut terminal), Ok(true));
        let mut late = runtime_frame("a", 9, 4);
        assert_eq!(apply_frame(&mut snapshot, &mut late), Ok(true));
        let ServiceEvent::Agent(AgentEvent::Daemon(DaemonEvent::TaskRuntimeChanged(runtime))) =
            late.event
        else {
            panic!("runtime event")
        };
        assert_eq!(runtime.active_transfers, Some(0));
        // 重连快照作为新 epoch 的唯一权威基线。
        let mut recovered = source_snapshot("b", 20);
        assert_eq!(
            apply_frame(&mut recovered, &mut runtime_frame("b", 21, 4)),
            Ok(true)
        );
        assert_eq!(
            apply_frame(&mut recovered, &mut runtime_frame("a", 7, 0)),
            Err(())
        );
        let SnapshotBody::Agent(body) = recovered.body else {
            panic!("agent snapshot")
        };
        assert_eq!(body.daemon.task_runtime["task"].active_transfers, Some(4));
    }
}
