//! 单实例激活通道：次实例把链接 / 激活请求交给主实例后退出。
//!
//! Unix 用 `<instance_dir>/activate.sock`，Windows 用命名管道
//! `\\.\pipe\fluxdown-desktop-<instance_dir FNV hash>`。消息为一行 JSON。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

/// 次实例 → 主实例的一次请求。
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivateMessage {
    #[serde(default)]
    pub urls: Vec<String>,
    #[serde(default)]
    pub files: Vec<PathBuf>,
    #[serde(default)]
    pub activate: bool,
}

/// 通道端点（socket 路径或管道名）。
#[derive(Debug, Clone)]
pub struct Endpoint(String);

impl Endpoint {
    #[must_use]
    pub fn for_instance_dir(instance_dir: &Path) -> Self {
        if cfg!(windows) {
            let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
            for byte in instance_dir.to_string_lossy().as_bytes() {
                hash ^= u64::from(*byte);
                hash = hash.wrapping_mul(0x0100_0000_01b3);
            }
            Self(format!(r"\\.\pipe\fluxdown-desktop-{hash:016x}"))
        } else {
            Self(instance_dir.join("activate.sock").display().to_string())
        }
    }
}

/// 次实例：尝试把消息交给主实例。返回 `false` 表示主实例不可达。
pub fn try_send_to_primary(endpoint: &Endpoint, message: &ActivateMessage) -> bool {
    let Ok(mut line) = serde_json::to_string(message) else {
        return false;
    };
    line.push('\n');
    send_line(endpoint, &line)
}

#[cfg(unix)]
fn send_line(endpoint: &Endpoint, line: &str) -> bool {
    use std::io::Write as _;
    let Ok(mut stream) = std::os::unix::net::UnixStream::connect(&endpoint.0) else {
        return false;
    };
    stream.write_all(line.as_bytes()).is_ok() && stream.flush().is_ok()
}

#[cfg(windows)]
fn send_line(endpoint: &Endpoint, line: &str) -> bool {
    use std::io::Write as _;
    let Ok(mut file) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&endpoint.0)
    else {
        return false;
    };
    file.write_all(line.as_bytes()).is_ok() && file.flush().is_ok()
}

/// 主实例：在 tokio 运行时上监听，收到的消息送入 `tx`。
pub async fn listen(endpoint: Endpoint, tx: mpsc::Sender<ActivateMessage>) {
    #[cfg(unix)]
    listen_unix(&endpoint.0, tx).await;
    #[cfg(windows)]
    listen_windows(&endpoint.0, tx).await;
}

#[cfg(unix)]
async fn listen_unix(path: &str, tx: mpsc::Sender<ActivateMessage>) {
    use tokio::io::{AsyncBufReadExt as _, BufReader};
    let _ = std::fs::remove_file(path);
    let Ok(listener) = tokio::net::UnixListener::bind(path) else {
        eprintln!("FluxDown desktop activation socket unavailable: {path}");
        return;
    };
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut line = String::new();
            if BufReader::new(stream).read_line(&mut line).await.is_ok()
                && let Ok(message) = serde_json::from_str::<ActivateMessage>(line.trim())
            {
                let _ = tx.send(message).await;
            }
        });
    }
}

#[cfg(windows)]
async fn listen_windows(name: &str, tx: mpsc::Sender<ActivateMessage>) {
    use tokio::io::{AsyncBufReadExt as _, BufReader};
    use tokio::net::windows::named_pipe::ServerOptions;
    let Ok(mut server) = ServerOptions::new().first_pipe_instance(true).create(name) else {
        eprintln!("FluxDown desktop activation pipe unavailable: {name}");
        return;
    };
    loop {
        if server.connect().await.is_err() {
            continue;
        }
        let connected = server;
        server = match ServerOptions::new().create(name) {
            Ok(next) => next,
            Err(_) => return,
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let mut line = String::new();
            if BufReader::new(connected).read_line(&mut line).await.is_ok()
                && let Ok(message) = serde_json::from_str::<ActivateMessage>(line.trim())
            {
                let _ = tx.send(message).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::ActivateMessage;

    #[test]
    fn message_round_trips_and_tolerates_missing_fields() {
        let message = ActivateMessage {
            urls: vec!["https://a/b".to_owned()],
            files: vec!["/tmp/x.torrent".into()],
            activate: true,
        };
        let json = serde_json::to_string(&message).expect("serialize");
        assert_eq!(
            serde_json::from_str::<ActivateMessage>(&json).expect("parse"),
            message
        );
        assert_eq!(
            serde_json::from_str::<ActivateMessage>("{}").expect("parse empty"),
            ActivateMessage::default()
        );
    }

    #[cfg(unix)]
    #[test]
    fn unix_channel_delivers_message_to_listener() {
        let dir = std::env::temp_dir().join(format!("fluxdown-ipc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let endpoint = super::Endpoint::for_instance_dir(&dir);
        assert!(!super::try_send_to_primary(
            &endpoint,
            &ActivateMessage::default()
        ));

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("runtime");
        let (tx, mut rx) = tokio::sync::mpsc::channel(4);
        runtime.spawn(super::listen(endpoint.clone(), tx));
        let sent = ActivateMessage {
            urls: vec!["magnet:?xt=abc".to_owned()],
            files: Vec::new(),
            activate: true,
        };
        let mut delivered = false;
        for _ in 0..50 {
            if super::try_send_to_primary(&endpoint, &sent) {
                delivered = true;
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(delivered);
        let received = runtime
            .block_on(async {
                tokio::time::timeout(std::time::Duration::from_secs(2), rx.recv()).await
            })
            .expect("timeout")
            .expect("message");
        assert_eq!(received, sent);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
