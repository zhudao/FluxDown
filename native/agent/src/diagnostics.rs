//! agent Doctor 探测、daemon 诊断聚合与专用修复命令。

use std::sync::Arc;

use serde_json::{Value, json};

use crate::daemon_client::DaemonClient;
use crate::state::StateStore;

pub struct DiagnosticsService {
    daemon: Arc<DaemonClient>,
    state: Arc<StateStore>,
}

impl DiagnosticsService {
    #[must_use]
    pub fn new(daemon: Arc<DaemonClient>, state: Arc<StateStore>) -> Self {
        Self { daemon, state }
    }

    pub async fn run(&self) -> Result<Value, DiagnosticsError> {
        let state_readable = self.state.load().await.is_ok();
        let daemon = self
            .daemon
            .call::<Value, Value>(fluxdown_protocol::method::DAEMON_DIAGNOSTICS_DESCRIBE, None)
            .await
            .unwrap_or_else(|error| json!({ "error": format!("{:?}", error.code) }));
        Ok(json!({
            "agent": {
                "stateReadable": state_readable,
                "dataDir": self.state.data_dir().display().to_string(),
            },
            "daemon": daemon,
        }))
    }

    pub async fn repair(&self, action: &str) -> Result<Value, DiagnosticsError> {
        let method = match action {
            "refreshTrackers" => fluxdown_protocol::method::DAEMON_BT_TRACKER_SUBSCRIPTION_REFRESH,
            "refreshEd2kServers" => {
                fluxdown_protocol::method::DAEMON_ED2K_SERVER_SUBSCRIPTION_REFRESH
            }
            _ => return Err(DiagnosticsError::InvalidAction(action.to_owned())),
        };
        self.daemon
            .call::<Value, Value>(method, None)
            .await
            .map_err(DiagnosticsError::Daemon)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DiagnosticsError {
    #[error("invalid diagnostic repair: {0}")]
    InvalidAction(String),
    #[error("daemon diagnostic RPC failed: {0:?}")]
    Daemon(fluxdown_protocol::RpcErrorData),
    #[error(transparent)]
    State(#[from] crate::state::StateError),
}
