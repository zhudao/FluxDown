use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fluxdown_protocol::{AgentEvent, AgentSnapshot, ServiceEvent, SettingOwner};

pub type PortFuture<T> =
    Pin<Box<dyn Future<Output = Result<T, fluxdown_protocol::RpcErrorData>> + Send + 'static>>;

pub enum SettingsCommand {
    PatchDaemon(fluxdown_protocol::DaemonConfigPatch),
    ProxyTest(serde_json::Value),
    PatchGateway(serde_json::Value),
    PatchPreferences(BTreeMap<String, serde_json::Value>),
    PatchLocalPreferences(BTreeMap<String, serde_json::Value>),
    SetSyncEnabled(bool),
    SyncNow,
    RunDiagnostics,
    RepairDiagnostics(String),
}

pub enum SettingsResult {
    Unit,
    Value(serde_json::Value),
}

pub trait SettingsPort: Send + Sync {
    fn execute(&self, command: SettingsCommand) -> PortFuture<SettingsResult>;
}

pub struct SettingsController {
    port: Arc<dyn SettingsPort>,
    daemon: fluxdown_protocol::DaemonConfigSnapshot,
    gateway: fluxdown_protocol::GatewayStatusDto,
    preferences: fluxdown_protocol::AgentPreferencesDto,
    sync: fluxdown_protocol::SyncStatusDto,
    pending: BTreeSet<String>,
    stale: bool,
}

impl SettingsController {
    #[must_use]
    pub fn new(port: Arc<dyn SettingsPort>) -> Self {
        Self {
            port,
            daemon: Default::default(),
            gateway: Default::default(),
            preferences: Default::default(),
            sync: Default::default(),
            pending: BTreeSet::new(),
            stale: true,
        }
    }

    pub fn replace_snapshot(&mut self, snapshot: &AgentSnapshot) {
        self.daemon.clone_from(&snapshot.daemon.config);
        self.gateway.clone_from(&snapshot.gateway);
        self.preferences.clone_from(&snapshot.preferences);
        self.sync.clone_from(&snapshot.sync);
        self.pending.clear();
        self.stale = false;
    }

    pub fn apply_event(&mut self, event: &ServiceEvent) {
        let ServiceEvent::Agent(event) = event else {
            return;
        };
        match event {
            AgentEvent::Daemon(fluxdown_protocol::DaemonEvent::ConfigChanged(config)) => {
                self.daemon.clone_from(config)
            }
            AgentEvent::GatewayChanged(gateway) => self.gateway.clone_from(gateway),
            AgentEvent::PreferencesChanged(preferences) => self.preferences.clone_from(preferences),
            AgentEvent::SyncChanged(sync) => self.sync.clone_from(sync),
            AgentEvent::DaemonSnapshotReplaced(snapshot) => {
                self.daemon.clone_from(&snapshot.config);
                self.stale = false;
            }
            AgentEvent::DaemonConnectionChanged(connected) => self.stale = !connected,
            _ => return,
        }
        self.pending.clear();
    }

    pub fn mark_stale(&mut self) {
        self.stale = true;
        self.pending.clear();
    }

    pub fn bool_value(&self, key: &str, fallback: bool) -> bool {
        self.value(key)
            .and_then(|value| value.as_bool())
            .or_else(|| {
                self.preferences
                    .values
                    .get(key)
                    .and_then(serde_json::Value::as_bool)
            })
            .or_else(|| {
                self.daemon
                    .values
                    .get(key)
                    .map(|value| value == "true" || value == "1")
            })
            .unwrap_or(fallback)
    }

    #[must_use]
    pub fn value(&self, key: &str) -> Option<serde_json::Value> {
        let spec = fluxdown_protocol::setting_spec(key)?;
        match spec.owner {
            SettingOwner::Daemon => self
                .daemon
                .values
                .get(spec.storage_key)
                .and_then(|value| fluxdown_protocol::daemon_config_to_value(spec, value).ok()),
            SettingOwner::Agent | SettingOwner::Preferences => {
                self.preferences.values.get(key).cloned()
            }
            SettingOwner::Excluded => None,
        }
    }

    #[must_use]
    pub fn daemon_raw(&self, key: &str) -> Option<&str> {
        self.daemon.values.get(key).map(String::as_str)
    }

    #[must_use]
    pub fn gateway_bool(&self, key: &str) -> bool {
        match key {
            "takeoverEnabled" => self.gateway.takeover_enabled,
            "jsonrpcEnabled" => self.gateway.jsonrpc_enabled,
            "apiEnabled" => self.gateway.api_enabled,
            "mcpEnabled" => self.gateway.mcp_enabled,
            "corsEnabled" => self.gateway.cors_enabled,
            _ => false,
        }
    }

    pub fn set_gateway_bool(
        &mut self,
        key: &'static str,
        value: bool,
    ) -> PortFuture<SettingsResult> {
        if self.stale {
            return Box::pin(async { Err(unavailable()) });
        }
        let mut values = serde_json::Map::new();
        values.insert(key.to_owned(), serde_json::Value::Bool(value));
        self.port
            .execute(SettingsCommand::PatchGateway(serde_json::Value::Object(
                values,
            )))
    }

    pub fn set_daemon_raw(
        &mut self,
        key: &'static str,
        value: String,
    ) -> PortFuture<SettingsResult> {
        if self.stale {
            return Box::pin(async { Err(unavailable()) });
        }
        self.daemon.values.insert(key.to_owned(), value.clone());
        self.port.execute(SettingsCommand::PatchDaemon(
            fluxdown_protocol::DaemonConfigPatch {
                expected_revision: self.daemon.revision,
                values: BTreeMap::from([(key.to_owned(), value)]),
            },
        ))
    }

    pub fn execute(&self, command: SettingsCommand) -> PortFuture<SettingsResult> {
        self.port.execute(command)
    }

    #[must_use]
    pub fn is_read_only(&self) -> bool {
        self.stale
    }

    pub fn set_bool(&mut self, key: String, value: bool) -> PortFuture<SettingsResult> {
        self.set_value(key, serde_json::Value::Bool(value))
    }

    pub fn set_value(
        &mut self,
        key: String,
        value: serde_json::Value,
    ) -> PortFuture<SettingsResult> {
        if self.stale {
            return Box::pin(async { Err(unavailable()) });
        }
        let Some(spec) = fluxdown_protocol::setting_spec(&key) else {
            self.pending.insert(key.clone());
            self.preferences.values.insert(key.clone(), value.clone());
            return self
                .port
                .execute(SettingsCommand::PatchLocalPreferences(BTreeMap::from([(
                    key, value,
                )])));
        };
        if let Err(_error) = fluxdown_protocol::validate_value(spec.key, &value) {
            return Box::pin(async { Err(invalid_argument()) });
        }
        self.pending.insert(key.clone());
        match spec.owner {
            SettingOwner::Daemon => {
                let config_value = match fluxdown_protocol::value_to_daemon_config(spec, &value) {
                    Ok(value) => value,
                    Err(_) => return Box::pin(async { Err(invalid_argument()) }),
                };
                self.daemon
                    .values
                    .insert(spec.storage_key.to_owned(), config_value.clone());
                self.port.execute(SettingsCommand::PatchDaemon(
                    fluxdown_protocol::DaemonConfigPatch {
                        expected_revision: self.daemon.revision,
                        values: BTreeMap::from([(spec.storage_key.to_owned(), config_value)]),
                    },
                ))
            }
            SettingOwner::Agent | SettingOwner::Preferences => {
                self.preferences.values.insert(key.clone(), value.clone());
                self.port
                    .execute(SettingsCommand::PatchPreferences(BTreeMap::from([(
                        key, value,
                    )])))
            }
            SettingOwner::Excluded => Box::pin(async { Err(invalid_argument()) }),
        }
    }
}

fn unavailable() -> fluxdown_protocol::RpcErrorData {
    fluxdown_protocol::RpcErrorData::new(fluxdown_protocol::ApplicationErrorCode::Unavailable, true)
}

fn invalid_argument() -> fluxdown_protocol::RpcErrorData {
    fluxdown_protocol::RpcErrorData::new(
        fluxdown_protocol::ApplicationErrorCode::InvalidArgument,
        false,
    )
}
