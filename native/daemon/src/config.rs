//! daemon 进程环境配置与 loopback 绑定约束。

use std::collections::{BTreeMap, HashMap};
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

/// daemon 默认控制端口。
pub const DEFAULT_DAEMON_BIND: &str = "127.0.0.1:17801";

/// daemon 进程级配置。
#[derive(Clone, Debug)]
pub struct DaemonConfig {
    pub bind_addr: SocketAddr,
    pub data_dir_override: Option<PathBuf>,
    pub database_url: Option<String>,
    pub token_file_override: Option<PathBuf>,
}

/// daemon 配置错误。
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("FLUXDOWN_DAEMON_BIND is invalid: {0}")]
    InvalidBind(String),
    #[error("FLUXDOWN_DAEMON_BIND must be loopback, got {0}")]
    NonLoopback(IpAddr),
    #[error("unknown or daemon-private config field: {0}")]
    UnknownField(String),
    #[error("invalid config value for {field}: {message}")]
    InvalidValue { field: String, message: String },
}

impl DaemonConfig {
    /// 从环境读取配置并拒绝非 loopback 绑定。
    pub fn from_env() -> Result<Self, ConfigError> {
        let bind_text = std::env::var("FLUXDOWN_DAEMON_BIND")
            .unwrap_or_else(|_| DEFAULT_DAEMON_BIND.to_owned());
        let bind_addr = bind_text
            .parse::<SocketAddr>()
            .map_err(|error| ConfigError::InvalidBind(error.to_string()))?;
        if !bind_addr.ip().is_loopback() {
            return Err(ConfigError::NonLoopback(bind_addr.ip()));
        }
        Ok(Self {
            bind_addr,
            data_dir_override: std::env::var_os("FLUXDOWN_DATA_DIR").map(PathBuf::from),
            database_url: std::env::var("FLUXDOWN_DATABASE_URL")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            token_file_override: std::env::var_os("FLUXDOWN_DAEMON_TOKEN_FILE").map(PathBuf::from),
        })
    }
}

/// 校验并规范化客户端可写的 daemon 设置。
pub fn validate_config_patch(
    values: &BTreeMap<String, String>,
) -> Result<BTreeMap<String, String>, ConfigError> {
    values
        .iter()
        .map(|(key, value)| {
            let normalized = match key.as_str() {
                "max_concurrent_tasks" => normalize_integer(key, value, 1, 1024)?,
                "speed_limit_bytes" | "upload_limit_bytes" => {
                    normalize_integer(key, value, 0, i64::MAX)?
                }
                "default_segments" => normalize_integer(key, value, 0, 64)?,
                "auto_max_connections" => normalize_integer(key, value, 0, 128)?,
                "cdn_max_nodes" => normalize_integer(key, value, 0, 8)?,
                "max_auto_retries" => normalize_integer(key, value, -1, 20)?,
                "auto_retry_delay_secs" => normalize_integer(key, value, 0, 86_400)?,
                "bt_port_start" | "bt_port_end" => normalize_integer(key, value, 1, 65_535)?,
                "bt_seed_time_limit_minutes"
                | "bt_seed_inactive_time_limit_minutes"
                | "bt_seed_max_active" => normalize_integer(key, value, 0, i64::MAX)?,
                "bt_seed_ratio_limit" | "bt_seed_post_ratio_limit" => {
                    normalize_float(key, value, 0.0)?
                }
                "cdn_multi_enabled"
                | "auto_resume_on_start"
                | "use_server_time"
                | "bt_enable_dht"
                | "bt_enable_upnp"
                | "bt_tracker_sub_enabled"
                | "ed2k_server_sub_enabled"
                | "ed2k_enable_kad"
                | "ed2k_enable_upnp" => normalize_boolean(key, value)?,
                "file_exists_behavior" => normalize_enum(key, value, &["rename", "overwrite"])?,
                "file_missing_action" => normalize_enum(key, value, &["keep", "delete"])?,
                "bt_seed_limit_operator" => normalize_enum(key, value, &["or", "and"])?,
                "bt_seed_then_action" => {
                    normalize_enum(key, value, &["stop", "delete", "delete_files"])?
                }
                "bt_mse_mode" => normalize_enum(key, value, &["disabled", "enabled", "forced"])?,
                "proxy_mode"
                | "proxy_type"
                | "proxy_host"
                | "proxy_port"
                | "proxy_username"
                | "proxy_password"
                | "proxy_no_list"
                | "default_save_dir"
                | "global_user_agent"
                | "bt_custom_trackers"
                | "bt_tracker_sub_urls"
                | "ed2k_server_sub_urls"
                | "ed2k_server_list"
                | "ed2k_nodes_dat_url"
                | "webhook.endpoints" => value.trim().to_owned(),
                _ => return Err(ConfigError::UnknownField(key.clone())),
            };
            Ok((key.clone(), normalized))
        })
        .collect()
}

fn normalize_integer(
    field: &str,
    value: &str,
    minimum: i64,
    maximum: i64,
) -> Result<String, ConfigError> {
    let parsed = value
        .trim()
        .parse::<i64>()
        .map_err(|error| invalid_value(field, error.to_string()))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(invalid_value(
            field,
            format!("must be between {minimum} and {maximum}"),
        ));
    }
    Ok(parsed.to_string())
}

fn normalize_float(field: &str, value: &str, minimum: f64) -> Result<String, ConfigError> {
    let parsed = value
        .trim()
        .parse::<f64>()
        .map_err(|error| invalid_value(field, error.to_string()))?;
    if !parsed.is_finite() || parsed < minimum {
        return Err(invalid_value(
            field,
            format!("must be finite and >= {minimum}"),
        ));
    }
    Ok(parsed.to_string())
}

fn normalize_boolean(field: &str, value: &str) -> Result<String, ConfigError> {
    match value.trim() {
        "true" | "1" => Ok("true".to_owned()),
        "false" | "0" => Ok("false".to_owned()),
        _ => Err(invalid_value(field, "must be true or false".to_owned())),
    }
}

fn normalize_enum(field: &str, value: &str, allowed: &[&str]) -> Result<String, ConfigError> {
    let normalized = value.trim().to_ascii_lowercase();
    if allowed.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        Err(invalid_value(
            field,
            format!("must be one of {}", allowed.join(", ")),
        ))
    }
}

fn invalid_value(field: &str, message: String) -> ConfigError {
    ConfigError::InvalidValue {
        field: field.to_owned(),
        message,
    }
}

/// 从完整 engine config 中只投影 daemon UI 可见且非敏感的键。
#[must_use]
pub fn public_config_values(all: &HashMap<String, String>) -> BTreeMap<String, String> {
    all.iter()
        .filter_map(|(key, value)| {
            let single = BTreeMap::from([(key.clone(), value.clone())]);
            validate_config_patch(&single)
                .ok()
                .map(|_| (key.clone(), value.clone()))
        })
        .collect()
}

/// 从持久化配置构建完整 BT 运行配置。
#[must_use]
pub fn bt_config_from_map(
    cfg: &HashMap<String, String>,
) -> fluxdown_engine::bt_downloader::BtConfig {
    use fluxdown_engine::bt_downloader::BtMseMode;
    use fluxdown_engine::bt_seeding::SeedingLimitOperator;

    let subscription_enabled = cfg
        .get("bt_tracker_sub_enabled")
        .map(|value| value == "true")
        .unwrap_or(true);
    fluxdown_engine::bt_downloader::BtConfig {
        enable_dht: cfg
            .get("bt_enable_dht")
            .map(|value| value == "true")
            .unwrap_or(true),
        enable_upnp: cfg
            .get("bt_enable_upnp")
            .map(|value| value == "true")
            .unwrap_or(true),
        port_start: cfg
            .get("bt_port_start")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(6881),
        port_end: cfg
            .get("bt_port_end")
            .and_then(|value| value.parse::<u16>().ok())
            .unwrap_or(6891),
        custom_trackers: cfg.get("bt_custom_trackers").cloned().unwrap_or_default(),
        subscription_trackers: if subscription_enabled {
            cfg.get("bt_tracker_sub_cache").cloned().unwrap_or_default()
        } else {
            String::new()
        },
        seed_ratio_limit: cfg
            .get("bt_seed_ratio_limit")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        seed_post_ratio_limit: cfg
            .get("bt_seed_post_ratio_limit")
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.0),
        seed_time_limit_minutes: cfg
            .get("bt_seed_time_limit_minutes")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0),
        seed_inactive_time_limit_minutes: cfg
            .get("bt_seed_inactive_time_limit_minutes")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0),
        seed_limit_operator: cfg
            .get("bt_seed_limit_operator")
            .map(|value| {
                if value.eq_ignore_ascii_case("and") {
                    SeedingLimitOperator::And
                } else {
                    SeedingLimitOperator::Or
                }
            })
            .unwrap_or(SeedingLimitOperator::Or),
        seed_then_action: cfg
            .get("bt_seed_then_action")
            .cloned()
            .unwrap_or_else(|| "stop".to_owned()),
        seed_max_active: cfg
            .get("bt_seed_max_active")
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(0),
        mse_mode: cfg
            .get("bt_mse_mode")
            .map(String::as_str)
            .map(BtMseMode::from)
            .unwrap_or_default(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    use super::validate_config_patch;

    #[test]
    fn loopback_predicate_rejects_public_bind() {
        let loopback = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 17801);
        let public = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 17801);
        assert!(loopback.ip().is_loopback());
        assert!(!public.ip().is_loopback());
    }

    #[test]
    fn sync_catalog_semantics_normalize_without_losing_flutter_values() {
        let values = BTreeMap::from([
            ("auto_max_connections".to_owned(), "0".to_owned()),
            ("max_auto_retries".to_owned(), "-1".to_owned()),
            ("auto_resume_on_start".to_owned(), "true".to_owned()),
            ("ed2k_enable_upnp".to_owned(), "false".to_owned()),
            ("ed2k_server_list".to_owned(), "ed2k://server".to_owned()),
            ("bt_seed_then_action".to_owned(), "delete_files".to_owned()),
            ("bt_mse_mode".to_owned(), "enabled".to_owned()),
        ]);
        let normalized = validate_config_patch(&values).expect("valid synced config");
        assert_eq!(normalized, values);
    }
}
