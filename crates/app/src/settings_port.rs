use std::sync::Arc;

use fluxdown_ui_settings::{PortFuture, SettingsCommand, SettingsPort, SettingsResult};
use serde_json::{Value, json};

use crate::agent_client::AgentClient;

pub struct AgentSettingsPort {
    client: Arc<AgentClient>,
}

impl AgentSettingsPort {
    #[must_use]
    pub fn new(client: Arc<AgentClient>) -> Self {
        Self { client }
    }
}

impl SettingsPort for AgentSettingsPort {
    fn execute(&self, command: SettingsCommand) -> PortFuture<SettingsResult> {
        let client = self.client.clone();
        Box::pin(async move {
            let (method, params) = match command {
                SettingsCommand::PatchDaemon(patch) => (
                    fluxdown_protocol::method::DAEMON_CONFIG_PATCH,
                    serialize(patch)?,
                ),
                SettingsCommand::ProxyTest(params) => {
                    (fluxdown_protocol::method::DAEMON_CONFIG_PROXY_TEST, params)
                }
                SettingsCommand::PatchGateway(params) => {
                    (fluxdown_protocol::method::AGENT_GATEWAY_PATCH, params)
                }
                SettingsCommand::PatchPreferences(values) => (
                    fluxdown_protocol::method::AGENT_PREFERENCES_PATCH,
                    json!({ "values": values }),
                ),
                SettingsCommand::PatchLocalPreferences(values) => (
                    fluxdown_protocol::method::AGENT_PREFERENCES_PATCH,
                    json!({ "values": values, "sync": false }),
                ),
                SettingsCommand::SetSyncEnabled(true) => {
                    (fluxdown_protocol::method::AGENT_SYNC_ENABLE, json!({}))
                }
                SettingsCommand::SetSyncEnabled(false) => {
                    (fluxdown_protocol::method::AGENT_SYNC_DISABLE, json!({}))
                }
                SettingsCommand::SyncNow => (fluxdown_protocol::method::AGENT_SYNC_NOW, json!({})),
                SettingsCommand::RunDiagnostics => {
                    (fluxdown_protocol::method::AGENT_DIAGNOSTICS_RUN, json!({}))
                }
                SettingsCommand::RepairDiagnostics(action) => (
                    fluxdown_protocol::method::AGENT_DIAGNOSTICS_REPAIR,
                    json!({ "action": action }),
                ),
            };
            let value: Value = client.call(method, Some(params)).await?;
            Ok(if value == json!({ "ok": true }) {
                SettingsResult::Unit
            } else {
                SettingsResult::Value(value)
            })
        })
    }
}

fn serialize<T: serde::Serialize>(value: T) -> Result<Value, fluxdown_protocol::RpcErrorData> {
    serde_json::to_value(value).map_err(|_| {
        fluxdown_protocol::RpcErrorData::new(
            fluxdown_protocol::ApplicationErrorCode::Internal,
            false,
        )
    })
}
