//! GPUI 对同级 `fluxdown-agent` 的单飞启动与异步回收。

use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use tokio::net::TcpStream;
use tokio::sync::Mutex;

#[derive(Default)]
struct BootstrapState {
    generation: u64,
    running: bool,
    reapers: Vec<tokio::task::JoinHandle<()>>,
}

pub struct ServiceBootstrap {
    state: Arc<Mutex<BootstrapState>>,
}

impl ServiceBootstrap {
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(BootstrapState::default())),
        }
    }

    /// 仅由 connection-refused/no-listener 路径调用。
    ///
    /// 先探测目标端口。这样当另一桌面进程或直接启动的 agent 正在完成
    /// 初始化（尚未写出 bearer）时，不会反复拉起会立即因独占锁退出的子进程。
    pub async fn ensure_running(&self, rpc_url: &str) -> Result<(), BootstrapError> {
        let mut state = self.state.lock().await;
        state.reapers.retain(|task| !task.is_finished());
        if state.running {
            return Ok(());
        }
        if agent_is_listening(rpc_url).await? {
            return Ok(());
        }
        let executable = agent_executable()?;
        let mut command = std::process::Command::new(executable);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        set_no_console_window(&mut command);
        let mut child = tokio::process::Command::from(command)
            .spawn()
            .map_err(|error| BootstrapError::Spawn(format!("{error:#}")))?;
        state.generation = state.generation.saturating_add(1);
        let generation = state.generation;
        state.running = true;
        let bootstrap_state = self.state.clone();
        state.reapers.push(tokio::spawn(async move {
            let _ = child.wait().await;
            let mut state = bootstrap_state.lock().await;
            if state.generation == generation {
                state.running = false;
            }
        }));
        Ok(())
    }
}

async fn agent_is_listening(rpc_url: &str) -> Result<bool, BootstrapError> {
    let target = agent_socket_target(rpc_url)?;
    match tokio::time::timeout(Duration::from_millis(250), TcpStream::connect(target)).await {
        Ok(Ok(_)) => Ok(true),
        Ok(Err(error))
            if matches!(
                error.kind(),
                std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
            ) =>
        {
            Ok(false)
        }
        Ok(Err(error)) => Err(BootstrapError::Probe(error.to_string())),
        Err(_) => Err(BootstrapError::Probe(
            "agent listener probe timed out".to_owned(),
        )),
    }
}

fn agent_socket_target(rpc_url: &str) -> Result<String, BootstrapError> {
    let uri = rpc_url
        .parse::<tokio_tungstenite::tungstenite::http::Uri>()
        .map_err(|error| BootstrapError::Probe(error.to_string()))?;
    let authority = uri
        .authority()
        .ok_or_else(|| BootstrapError::Probe("agent URL has no authority".to_owned()))?;
    let port = authority
        .port_u16()
        .unwrap_or_else(|| match uri.scheme_str() {
            Some("wss") | Some("https") => 443,
            _ => 80,
        });
    let host = authority
        .host()
        .trim_start_matches('[')
        .trim_end_matches(']');
    if host.parse::<std::net::Ipv6Addr>().is_ok() {
        Ok(format!("[{host}]:{port}"))
    } else {
        Ok(format!("{host}:{port}"))
    }
}
fn agent_executable() -> Result<PathBuf, std::io::Error> {
    if let Some(path) = std::env::var_os("FLUXDOWN_AGENT_BIN") {
        return Ok(PathBuf::from(path));
    }
    let current = std::env::current_exe()?;
    Ok(current.with_file_name(if cfg!(windows) {
        "fluxdown-agent.exe"
    } else {
        "fluxdown-agent"
    }))
}

#[cfg(windows)]
fn set_no_console_window(command: &mut std::process::Command) {
    use std::os::windows::process::CommandExt;
    command.creation_flags(0x0800_0000);
}

#[cfg(not(windows))]
fn set_no_console_window(_command: &mut std::process::Command) {}

#[derive(Debug, thiserror::Error)]
pub enum BootstrapError {
    #[error("could not locate fluxdown-agent: {0}")]
    Locate(#[from] std::io::Error),
    #[error("could not spawn fluxdown-agent: {0}")]
    Spawn(String),
    #[error("could not probe fluxdown-agent listener: {0}")]
    Probe(String),
}
#[cfg(test)]
mod tests {
    use super::agent_socket_target;

    #[test]
    fn listener_probe_formats_ipv6_authority_once() {
        assert_eq!(
            agent_socket_target("ws://[::1]:17800/rpc").expect("IPv6 target"),
            "[::1]:17800"
        );
    }
}
