//! 浏览器/NMH 捕获确认事务：内存有界、单次消费且不持久化敏感请求上下文。

use std::collections::VecDeque;
use std::sync::Arc;

use fluxdown_protocol::{
    AgentEvent, CreateTaskRequest, DaemonCreateTaskParams, DownloadRequest, PendingCaptureDto,
};
use serde_json::{Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::daemon_client::DaemonClient;
use crate::event_hub::AgentEventHub;

const CAPTURE_CAPACITY: usize = 64;

struct CaptureTransaction {
    public: PendingCaptureDto,
    request: DownloadRequest,
}

pub struct CaptureService {
    daemon: Arc<DaemonClient>,
    events: AgentEventHub,
    pending: Mutex<VecDeque<CaptureTransaction>>,
}

impl CaptureService {
    #[must_use]
    pub fn new(daemon: Arc<DaemonClient>, events: AgentEventHub) -> Self {
        Self {
            daemon,
            events,
            pending: Mutex::new(VecDeque::with_capacity(CAPTURE_CAPACITY)),
        }
    }

    /// 静默策略直接提交 daemon；否则排入确认队列并在首项时唤起官方 UI。
    pub async fn submit(
        &self,
        request: DownloadRequest,
        silent: bool,
    ) -> Result<Value, CaptureError> {
        if silent {
            return self.create(request, true).await;
        }
        let public = PendingCaptureDto {
            transaction_id: Uuid::new_v4().to_string(),
            url: request.url.clone(),
            file_name: request.filename.clone(),
            created_at_unix_ms: now_unix_ms(),
        };
        let first = {
            let mut pending = self.pending.lock().await;
            if pending.len() >= CAPTURE_CAPACITY {
                return Err(CaptureError::Full);
            }
            let first = pending.is_empty();
            pending.push_back(CaptureTransaction {
                public: public.clone(),
                request,
            });
            first
        };
        self.publish().await;
        if first && let Err(error) = crate::platform::launch_desktop_once() {
            tracing::warn!(error = %error, "could not launch desktop for pending capture");
        }
        Ok(json!({ "transactionId": public.transaction_id }))
    }

    pub async fn list(&self) -> Vec<PendingCaptureDto> {
        self.pending
            .lock()
            .await
            .iter()
            .map(|transaction| transaction.public.clone())
            .collect()
    }

    /// 确认/拒绝均只消费一次。
    pub async fn resolve(
        &self,
        transaction_id: &str,
        accepted: bool,
    ) -> Result<Value, CaptureError> {
        let transaction = {
            let mut pending = self.pending.lock().await;
            let index = pending
                .iter()
                .position(|transaction| transaction.public.transaction_id == transaction_id)
                .ok_or(CaptureError::NotFound)?;
            pending.remove(index).ok_or(CaptureError::NotFound)?
        };
        self.publish().await;
        if accepted {
            self.create(transaction.request, false).await
        } else {
            Ok(json!({ "accepted": false }))
        }
    }

    async fn create(
        &self,
        request: DownloadRequest,
        unattended: bool,
    ) -> Result<Value, CaptureError> {
        let create = serde_json::from_value::<CreateTaskRequest>(json!({
            "url": request.url,
            "fileName": request.filename,
            "saveDir": request.save_dir,
            "referrer": request.referrer,
            "cookies": request.cookies,
            "headers": request.headers,
            "method": request.method,
            "body": request.body,
            "audioUrl": request.audio_url,
        }))?;
        self.daemon
            .call(
                fluxdown_protocol::method::DAEMON_TASK_CREATE,
                Some(DaemonCreateTaskParams {
                    request: create,
                    torrent_blob_id: None,
                    unattended,
                }),
            )
            .await
            .map_err(CaptureError::Daemon)
    }

    async fn publish(&self) {
        self.events
            .publish(AgentEvent::PendingCapturesChanged(self.list().await));
    }
}

fn now_unix_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().min(i64::MAX as u128) as i64
        })
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("capture confirmation queue is full")]
    Full,
    #[error("capture transaction not found")]
    NotFound,
    #[error("daemon capture create failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Platform(#[from] crate::platform::PlatformError),
}
