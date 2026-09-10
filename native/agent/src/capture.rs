//! 浏览器/NMH 捕获确认事务：内存有界、单次消费且不持久化敏感请求上下文。

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

use fluxdown_protocol::{
    AgentEvent, CaptureOverridesDto, CreateTaskRequest, DaemonCreateTaskParams, DownloadRequest,
    PendingCaptureDto,
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
    /// 已连接并声明 `client.selections` 能力的 UI 客户端数量（与 `GatewayService` 共享）。
    ui_clients: Arc<AtomicUsize>,
}

impl CaptureService {
    #[must_use]
    pub fn new(
        daemon: Arc<DaemonClient>,
        events: AgentEventHub,
        ui_clients: Arc<AtomicUsize>,
    ) -> Self {
        Self {
            daemon,
            events,
            pending: Mutex::new(VecDeque::with_capacity(CAPTURE_CAPACITY)),
            ui_clients,
        }
    }

    /// 静默策略直接提交 daemon；否则排入确认队列并在首项时唤起官方 UI。
    pub async fn submit(
        &self,
        request: DownloadRequest,
        silent: bool,
    ) -> Result<Value, CaptureError> {
        if silent {
            return self.create(request, None, true, None).await;
        }
        let public = PendingCaptureDto {
            transaction_id: Uuid::new_v4().to_string(),
            url: request.url.clone(),
            file_name: request.filename.clone(),
            created_at_unix_ms: now_unix_ms(),
            file_size: request.file_size.unwrap_or(0).max(0),
            referrer: request.referrer.clone(),
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
        if first && let Err(error) = crate::platform::launch_desktop_for_capture(&self.ui_clients) {
            tracing::warn!(error = %error, "could not launch desktop for pending capture");
        }
        Ok(json!({ "transactionId": public.transaction_id }))
    }

    /// 用户选定的本机 `.torrent`（已上传为 daemon blob）直接建任务；
    /// `unattended` 为 true 时全选文件不弹选择框。
    pub async fn create_torrent(
        &self,
        request: DownloadRequest,
        torrent_blob_id: String,
        unattended: bool,
    ) -> Result<Value, CaptureError> {
        self.create(request, Some(torrent_blob_id), unattended, None)
            .await
    }

    pub async fn list(&self) -> Vec<PendingCaptureDto> {
        self.pending
            .lock()
            .await
            .iter()
            .map(|transaction| transaction.public.clone())
            .collect()
    }

    /// 确认/拒绝均只消费一次；`overrides` 仅在 `accepted` 时按非空字段覆盖原始捕获请求。
    pub async fn resolve(
        &self,
        transaction_id: &str,
        accepted: bool,
        overrides: Option<CaptureOverridesDto>,
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
            self.create(transaction.request, None, false, overrides)
                .await
        } else {
            Ok(json!({ "accepted": false }))
        }
    }

    async fn create(
        &self,
        request: DownloadRequest,
        torrent_blob_id: Option<String>,
        unattended: bool,
        overrides: Option<CaptureOverridesDto>,
    ) -> Result<Value, CaptureError> {
        let create = build_create_request(request, overrides.as_ref())?;
        self.daemon
            .call(
                fluxdown_protocol::method::DAEMON_TASK_CREATE,
                Some(DaemonCreateTaskParams {
                    request: create,
                    torrent_blob_id,
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

/// 按非空覆盖字段合并进原始捕获请求，构造 `daemon.task.create` 参数。
/// `queueId`/`segments` 不属于 [`DownloadRequest`]，只能通过覆盖指定。
fn build_create_request(
    mut request: DownloadRequest,
    overrides: Option<&CaptureOverridesDto>,
) -> Result<CreateTaskRequest, serde_json::Error> {
    let mut queue_id = String::new();
    let mut segments = 0_i32;
    if let Some(overrides) = overrides {
        if !overrides.save_dir.is_empty() {
            request.save_dir = overrides.save_dir.clone();
        }
        if !overrides.file_name.is_empty() {
            request.filename = overrides.file_name.clone();
        }
        queue_id = overrides.queue_id.clone();
        segments = overrides.segments;
    }
    serde_json::from_value(json!({
        "url": request.url,
        "fileName": request.filename,
        "saveDir": request.save_dir,
        "referrer": request.referrer,
        "cookies": request.cookies,
        "headers": request.headers,
        "method": request.method,
        "body": request.body,
        "audioUrl": request.audio_url,
        "queueId": queue_id,
        "segments": segments,
    }))
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

/// daemon 专用二进制上传端点（`POST /blobs/{torrents|plugins}`）的客户端。
///
/// 与 RPC 共用 daemon 的 bearer；base URL 由 RPC URL 换成 http(s) 并去掉路径。
pub struct DaemonBlobClient {
    base_url: reqwest::Url,
    bearer: String,
    http: reqwest::Client,
}

/// 上传的 blob 类型，对应 daemon 端点路径段。
#[derive(Clone, Copy, Debug)]
pub enum BlobKind {
    Torrent,
    Plugin,
}

impl BlobKind {
    fn path(self) -> &'static str {
        match self {
            Self::Torrent => "/blobs/torrents",
            Self::Plugin => "/blobs/plugins",
        }
    }
}

impl DaemonBlobClient {
    pub fn new(config: &crate::daemon_client::DaemonClientConfig) -> Result<Self, BlobError> {
        let mut base_url = reqwest::Url::parse(&config.rpc_url)
            .map_err(|error| BlobError::Url(error.to_string()))?;
        let scheme = match base_url.scheme() {
            "ws" | "http" => "http",
            "wss" | "https" => "https",
            other => return Err(BlobError::Url(format!("unsupported scheme {other}"))),
        };
        base_url
            .set_scheme(scheme)
            .map_err(|()| BlobError::Url("scheme is not settable".to_owned()))?;
        base_url.set_path("");
        base_url.set_query(None);
        base_url.set_fragment(None);
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(5))
            .timeout(std::time::Duration::from_secs(60))
            .build()?;
        Ok(Self {
            base_url,
            bearer: config.bearer.clone(),
            http,
        })
    }

    /// 上传字节并返回 daemon 分配的 `blobId`。
    pub async fn upload(&self, kind: BlobKind, bytes: Vec<u8>) -> Result<String, BlobError> {
        let url = self
            .base_url
            .join(kind.path())
            .map_err(|error| BlobError::Url(error.to_string()))?;
        let response = self
            .http
            .post(url)
            .bearer_auth(&self.bearer)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(bytes)
            .send()
            .await?;
        let status = response.status();
        if !status.is_success() {
            return Err(BlobError::Status(status.as_u16()));
        }
        let body = response.json::<Value>().await?;
        body.get("blobId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(str::to_owned)
            .ok_or(BlobError::Decode)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BlobError {
    #[error("daemon URL is invalid: {0}")]
    Url(String),
    #[error("daemon blob upload transport failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("daemon blob upload rejected with HTTP {0}")]
    Status(u16),
    #[error("daemon blob upload response has no blobId")]
    Decode,
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::CaptureOverridesDto;

    use super::{DownloadRequest, build_create_request};

    fn sample_request() -> DownloadRequest {
        DownloadRequest {
            url: "https://example.com/a.bin".to_owned(),
            filename: "a.bin".to_owned(),
            save_dir: String::new(),
            referrer: String::new(),
            cookies: String::new(),
            headers: None,
            file_size: None,
            mime_type: None,
            method: None,
            body: None,
            audio_url: None,
        }
    }

    #[test]
    fn resolve_overrides_apply_to_created_request() {
        let overrides = CaptureOverridesDto {
            save_dir: "/tmp/renamed".to_owned(),
            file_name: "renamed.bin".to_owned(),
            queue_id: "later".to_owned(),
            segments: 4,
        };
        let created =
            build_create_request(sample_request(), Some(&overrides)).expect("build create request");
        assert_eq!(created.file_name, "renamed.bin");
        assert_eq!(created.save_dir, "/tmp/renamed");
        assert_eq!(created.queue_id, "later");
        assert_eq!(created.segments, 4);
        assert_eq!(created.url, "https://example.com/a.bin");
    }

    #[test]
    fn missing_overrides_keep_original_request_and_leave_queue_and_segments_default() {
        let created = build_create_request(sample_request(), None).expect("build create request");
        assert_eq!(created.file_name, "a.bin");
        assert_eq!(created.save_dir, "");
        assert_eq!(created.queue_id, "");
        assert_eq!(created.segments, 0);
    }

    #[test]
    fn empty_override_fields_fall_back_to_original_request() {
        let overrides = CaptureOverridesDto::default();
        let created =
            build_create_request(sample_request(), Some(&overrides)).expect("build create request");
        assert_eq!(created.file_name, "a.bin");
        assert_eq!(created.save_dir, "");
    }
}
