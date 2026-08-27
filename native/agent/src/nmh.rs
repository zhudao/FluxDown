//! 浏览器 Native Messaging Host 与 agent 之间的长度帧 IPC 服务。

use std::sync::Arc;

use fluxdown_protocol::{DownloadRequest, TaskDto};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::capture::CaptureService;
use crate::daemon_client::DaemonClient;

const MAX_MESSAGE_SIZE: u32 = 1024 * 1024;
const MAX_BATCH_ITEMS: usize = 1000;
const MAX_COMPLETED_TASKS: usize = 10;

#[derive(Deserialize)]
struct PipeMessage {
    action: String,
    #[serde(default)]
    msg_id: u64,
    #[serde(flatten)]
    payload: Value,
}

#[derive(Deserialize)]
struct BatchDownloadPayload {
    items: Vec<DownloadRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskOperation {
    op: String,
    task_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskIdPayload {
    task_id: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TaskBrief {
    task_id: String,
    file_name: String,
    status: i32,
    downloaded_bytes: i64,
    total_bytes: i64,
    speed: i64,
    error_message: String,
    created_at: String,
}

#[derive(Serialize)]
struct PipeResponse {
    success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    msg_id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    tasks: Option<Vec<TaskBrief>>,
}

impl PipeResponse {
    fn ok(msg_id: u64, message: impl Into<String>) -> Self {
        Self {
            success: true,
            message: Some(message.into()),
            msg_id,
            tasks: None,
        }
    }

    fn error(msg_id: u64, message: impl Into<String>) -> Self {
        Self {
            success: false,
            message: Some(message.into()),
            msg_id,
            tasks: None,
        }
    }

    fn tasks(msg_id: u64, tasks: Vec<TaskBrief>) -> Self {
        Self {
            success: true,
            message: None,
            msg_id,
            tasks: Some(tasks),
        }
    }
}

#[derive(Clone)]
pub struct NmhService {
    daemon: Arc<DaemonClient>,
    capture: Arc<CaptureService>,
}

impl NmhService {
    #[must_use]
    pub fn new(daemon: Arc<DaemonClient>, capture: Arc<CaptureService>) -> Self {
        Self { daemon, capture }
    }

    pub async fn run(self, cancel: CancellationToken) -> Result<(), std::io::Error> {
        run_server(self, cancel).await
    }

    async fn dispatch(&self, message: PipeMessage) -> PipeResponse {
        match message.action.as_str() {
            "ping" => PipeResponse::ok(message.msg_id, "pong"),
            "download" => match serde_json::from_value::<DownloadRequest>(message.payload) {
                Ok(request) => match self.capture.submit(request, false).await {
                    Ok(_) => PipeResponse::ok(message.msg_id, "download accepted"),
                    Err(error) => PipeResponse::error(message.msg_id, error.to_string()),
                },
                Err(error) => PipeResponse::error(message.msg_id, error.to_string()),
            },
            "batch_download" => self.batch_download(message.msg_id, message.payload).await,
            "tasks" => self.task_list(message.msg_id).await,
            "task_op" => self.task_operation(message.msg_id, message.payload).await,
            "open_file" => {
                self.platform_action(message.msg_id, message.payload, false)
                    .await
            }
            "reveal_file" => {
                self.platform_action(message.msg_id, message.payload, true)
                    .await
            }
            other => PipeResponse::error(message.msg_id, format!("unknown action: {other}")),
        }
    }

    async fn batch_download(&self, msg_id: u64, payload: Value) -> PipeResponse {
        let batch = match serde_json::from_value::<BatchDownloadPayload>(payload) {
            Ok(batch) if !batch.items.is_empty() && batch.items.len() <= MAX_BATCH_ITEMS => batch,
            Ok(batch) => {
                return PipeResponse::error(
                    msg_id,
                    format!("invalid batch size: {}", batch.items.len()),
                );
            }
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let count = batch.items.len();
        for request in batch.items {
            if let Err(error) = self.capture.submit(request, false).await {
                return PipeResponse::error(msg_id, error.to_string());
            }
        }
        PipeResponse::ok(msg_id, format!("batch accepted ({count} items)"))
    }

    async fn task_list(&self, msg_id: u64) -> PipeResponse {
        let tasks = match self
            .daemon
            .call::<Value, Vec<TaskDto>>(fluxdown_protocol::method::DAEMON_TASK_LIST, None)
            .await
        {
            Ok(tasks) => tasks,
            Err(error) => return PipeResponse::error(msg_id, format!("{:?}", error.code)),
        };
        PipeResponse::tasks(msg_id, select_task_briefs(tasks))
    }

    async fn task_operation(&self, msg_id: u64, payload: Value) -> PipeResponse {
        let operation = match serde_json::from_value::<TaskOperation>(payload) {
            Ok(operation) => operation,
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let method = match operation.op.as_str() {
            "pause" => fluxdown_protocol::method::DAEMON_TASK_PAUSE,
            "resume" => fluxdown_protocol::method::DAEMON_TASK_RESUME,
            "remove" => fluxdown_protocol::method::DAEMON_TASK_DELETE,
            other => return PipeResponse::error(msg_id, format!("unknown task op: {other}")),
        };
        let params = if operation.op == "remove" {
            serde_json::json!({"taskId": operation.task_id, "deleteFiles": false})
        } else {
            serde_json::json!({"taskId": operation.task_id})
        };
        match self.daemon.call::<Value, Value>(method, Some(params)).await {
            Ok(_) => PipeResponse::ok(msg_id, "ok"),
            Err(error) => PipeResponse::error(msg_id, format!("{:?}", error.code)),
        }
    }

    async fn platform_action(&self, msg_id: u64, payload: Value, reveal: bool) -> PipeResponse {
        let request = match serde_json::from_value::<TaskIdPayload>(payload) {
            Ok(request) => request,
            Err(error) => return PipeResponse::error(msg_id, error.to_string()),
        };
        let task = match self
            .daemon
            .call::<Value, TaskDto>(
                fluxdown_protocol::method::DAEMON_TASK_GET,
                Some(serde_json::json!({"taskId": request.task_id})),
            )
            .await
        {
            Ok(task) => task,
            Err(error) => return PipeResponse::error(msg_id, format!("{:?}", error.code)),
        };
        if task.status != 3 {
            return PipeResponse::error(msg_id, "task is not completed");
        }
        let outcome = if reveal {
            crate::platform::reveal_task(&task)
        } else {
            crate::platform::open_task(&task)
        };
        match outcome {
            Ok(()) => PipeResponse::ok(msg_id, "ok"),
            Err(error) => PipeResponse::error(msg_id, error.to_string()),
        }
    }
}

fn select_task_briefs(tasks: Vec<TaskDto>) -> Vec<TaskBrief> {
    let (mut completed, active): (Vec<_>, Vec<_>) =
        tasks.into_iter().partition(|task| task.status == 3);
    completed
        .sort_by_key(|task| std::cmp::Reverse(task.created_at.parse::<i64>().unwrap_or_default()));
    completed.truncate(MAX_COMPLETED_TASKS);
    active
        .into_iter()
        .chain(completed)
        .map(|task| TaskBrief {
            task_id: task.task_id,
            file_name: task.file_name,
            status: task.status,
            downloaded_bytes: task.downloaded_bytes,
            total_bytes: task.total_bytes,
            speed: 0,
            error_message: task.error_message,
            created_at: task.created_at,
        })
        .collect()
}

async fn handle_stream<S>(mut stream: S, service: NmhService) -> Result<(), std::io::Error>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    loop {
        let mut length = [0_u8; 4];
        if let Err(error) = stream.read_exact(&mut length).await {
            if error.kind() == std::io::ErrorKind::UnexpectedEof {
                return Ok(());
            }
            return Err(error);
        }
        let length = u32::from_le_bytes(length);
        if length == 0 || length > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "native messaging frame is invalid",
            ));
        }
        let mut payload = vec![0_u8; length as usize];
        stream.read_exact(&mut payload).await?;
        let response = match serde_json::from_slice::<PipeMessage>(&payload) {
            Ok(message) => service.dispatch(message).await,
            Err(error) => PipeResponse::error(0, error.to_string()),
        };
        let bytes = serde_json::to_vec(&response).map_err(std::io::Error::other)?;
        let length =
            u32::try_from(bytes.len()).map_err(|error| std::io::Error::other(error.to_string()))?;
        stream.write_all(&length.to_le_bytes()).await?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
    }
}

#[cfg(unix)]
async fn run_server(service: NmhService, cancel: CancellationToken) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;

    let path = unix_socket_path();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    if tokio::fs::try_exists(&path).await? {
        match tokio::net::UnixStream::connect(&path).await {
            Ok(_) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::AddrInUse,
                    "native messaging socket is already active",
                ));
            }
            Err(_) => tokio::fs::remove_file(&path).await?,
        }
    }
    let listener = tokio::net::UnixListener::bind(&path)?;
    tokio::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).await?;
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                drop(listener);
                let _ = tokio::fs::remove_file(&path).await;
                return Ok(());
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted?;
                let service = service.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_stream(stream, service).await {
                        tracing::debug!(error = %error, "NMH socket closed");
                    }
                });
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |home| std::path::PathBuf::from(home).join(".local/share/fluxdown/fluxdown.sock"),
    )
}

#[cfg(target_os = "macos")]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("HOME").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |home| {
            std::path::PathBuf::from(home)
                .join("Library/Application Support/fluxdown/fluxdown.sock")
        },
    )
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn unix_socket_path() -> std::path::PathBuf {
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || std::path::PathBuf::from("/tmp/fluxdown.sock"),
        |dir| std::path::PathBuf::from(dir).join("fluxdown.sock"),
    )
}

#[cfg(windows)]
async fn run_server(service: NmhService, cancel: CancellationToken) -> Result<(), std::io::Error> {
    use tokio::net::windows::named_pipe::ServerOptions;

    const PIPE_NAME: &str = r"\\.\pipe\fluxdown";
    let mut first = true;
    loop {
        let server = ServerOptions::new()
            .first_pipe_instance(first)
            .create(PIPE_NAME)?;
        first = false;
        tokio::select! {
            _ = cancel.cancelled() => return Ok(()),
            connected = server.connect() => connected?,
        }
        let service = service.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_stream(server, service).await {
                tracing::debug!(error = %error, "NMH pipe closed");
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{MAX_BATCH_ITEMS, PipeMessage, select_task_briefs};

    #[test]
    fn task_panel_keeps_active_and_ten_recent_completions() {
        let tasks = (0..15)
            .map(|index| {
                serde_json::from_value(json!({
                    "taskId": format!("task-{index}"),
                    "url": "https://example.com/a",
                    "fileName": format!("{index}.bin"),
                    "saveDir": "/tmp",
                    "status": if index == 0 { 1 } else { 3 },
                    "downloadedBytes": 1,
                    "totalBytes": 1,
                    "errorMessage": "",
                    "createdAt": index.to_string(),
                    "proxyUrl": "",
                    "queueId": "main",
                    "checksum": ""
                }))
                .expect("task")
            })
            .collect();
        let selected = select_task_briefs(tasks);
        assert_eq!(selected.len(), 11);
        assert_eq!(selected[0].task_id, "task-0");
        assert_eq!(selected[1].task_id, "task-14");
    }

    #[test]
    fn flattened_native_message_preserves_download_payload() {
        let message = serde_json::from_value::<PipeMessage>(json!({
            "action": "download",
            "msg_id": 7,
            "url": "https://example.com/a"
        }))
        .expect("native message");
        assert_eq!(message.msg_id, 7);
        assert_eq!(message.payload["url"], "https://example.com/a");
        assert_eq!(MAX_BATCH_ITEMS, 1000);
    }
}
