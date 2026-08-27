//! 官方 agent 暴露给 UI 的无令牌 DTO。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// FluxCloud 用户状态。
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum CloudUserStatus {
    #[default]
    Active,
    Disabled,
    Pending,
}

/// FluxCloud 用户公开资料。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudUser {
    pub id: String,
    pub email: String,
    #[serde(default)]
    pub nickname: String,
    #[serde(default)]
    pub plan: String,
    #[serde(default)]
    pub status: CloudUserStatus,
    #[serde(default)]
    pub created_at: String,
    pub last_login_at: Option<String>,
    pub origin_id: Option<i64>,
    #[serde(default)]
    pub origin_id_changed: bool,
    pub membership_ordinal: Option<i64>,
}

/// 前向兼容的套餐权益集合。未知字段必须原样保留。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(transparent)]
pub struct Entitlements(pub BTreeMap<String, Value>);

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudPlanCampaignStage {
    pub label: String,
    pub price_minor: i64,
    pub quota: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudPlanCampaign {
    pub name: String,
    pub end_at: Option<String>,
    pub stages: Vec<CloudPlanCampaignStage>,
    pub sold_total: i64,
    pub stage_sold: Vec<i64>,
    pub current_stage_index: i64,
    pub effective_price_minor: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudPlan {
    pub code: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub badge: Option<String>,
    #[serde(default)]
    pub icon: String,
    #[serde(default)]
    pub color: String,
    #[serde(default = "default_badge_style")]
    pub badge_style: String,
    #[serde(default)]
    pub badge_color: String,
    #[serde(default)]
    pub badge_numbered: bool,
    #[serde(default = "default_badge_number_digits")]
    pub badge_number_digits: i64,
    #[serde(default)]
    pub price_minor: i64,
    #[serde(default = "default_currency")]
    pub currency: String,
    #[serde(default)]
    pub highlights: Vec<String>,
    #[serde(default, rename = "entitlements")]
    pub entitlements_raw: BTreeMap<String, Value>,
    #[serde(default)]
    pub sort: i64,
    pub campaign: Option<CloudPlanCampaign>,
    #[serde(default = "default_true")]
    pub purchasable: bool,
}

fn default_badge_style() -> String {
    "outline".to_owned()
}

fn default_badge_number_digits() -> i64 {
    4
}

fn default_currency() -> String {
    "CNY".to_owned()
}

fn default_true() -> bool {
    true
}

/// 当前账户资料与套餐快照。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudProfile {
    pub user: CloudUser,
    #[serde(default)]
    pub entitlements: Entitlements,
    pub current_plan: Option<CloudPlan>,
    #[serde(default)]
    pub purchase_credit_minor: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct OriginIdCheckResult {
    pub available: bool,
    pub reason: Option<String>,
}

/// 受信任设备公开投影。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudDevice {
    pub id: String,
    pub device_id: String,
    #[serde(default)]
    pub name: String,
    pub platform: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub last_seen_at: String,
    pub last_ip: Option<String>,
    pub app_version: Option<String>,
    #[serde(default)]
    pub is_online: bool,
    #[serde(default)]
    pub is_current: bool,
}

/// 无 access/refresh token 的本地会话视图。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentSessionDto {
    pub user: CloudUser,
    #[serde(default)]
    pub entitlements: Entitlements,
    pub current_plan: Option<CloudPlan>,
    pub device: CloudDevice,
}

/// 邮箱或设备验证步骤的无令牌元数据。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AuthVerificationDto {
    pub ttl_seconds: u64,
    #[serde(default)]
    pub will_replace_devices: bool,
}

/// 登录的本地安全结果。成功结果只包含无令牌会话。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentLoginResult {
    Ok {
        session: Box<AgentSessionDto>,
    },
    DeviceVerificationRequired {
        ttl_seconds: u64,
        #[serde(default)]
        will_replace_devices: bool,
    },
}

/// 购买订单。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudOrder {
    pub order_no: String,
    pub plan_code: String,
    #[serde(default)]
    pub plan_name: String,
    #[serde(default = "default_order_status")]
    pub status: String,
    #[serde(default)]
    pub amount_minor: i64,
    #[serde(default)]
    pub list_price_minor: i64,
    #[serde(default)]
    pub credit_minor: i64,
    pub upgrade_from_plan: Option<String>,
    #[serde(default = "default_currency")]
    pub currency: String,
    pub campaign_name: Option<String>,
    pub stage_label: Option<String>,
    pub referral_code: Option<String>,
    #[serde(default)]
    pub referral_discount_minor: i64,
    pub code_url: Option<String>,
    #[serde(default)]
    pub created_at: String,
    pub paid_at: Option<String>,
    #[serde(default)]
    pub expires_at: String,
}

fn default_order_status() -> String {
    "pending".to_owned()
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralRule {
    pub plan_code: String,
    pub plan_name: String,
    pub price_minor: i64,
    pub discount_minor: i64,
    pub reward_percent: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralSummary {
    pub enabled: bool,
    pub description: String,
    pub reward_enabled: bool,
    pub contact: String,
    pub invited_count: i64,
    pub pending_reward_minor: i64,
    pub paid_reward_minor: i64,
    pub total_reward_minor: i64,
    pub rules: Vec<CloudReferralRule>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralCode {
    pub id: String,
    pub code: String,
    pub paid_order_count: i64,
    pub reward_minor: i64,
    pub created_at: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralCodesResult {
    pub total: i64,
    pub items: Vec<CloudReferralCode>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralRecord {
    pub id: String,
    pub buyer_label: String,
    pub order_amount_minor: i64,
    pub reward_minor: i64,
    pub reward_percent: i64,
    pub status: String,
    pub created_at: String,
    pub paid_at: Option<String>,
    pub referral_code: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralRecordsResult {
    pub total: i64,
    pub items: Vec<CloudReferralRecord>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct CloudReferralValidateResult {
    pub valid: bool,
    pub discount_minor: i64,
    pub reason: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub enum RemoteTaskStatus {
    Accepted,
    Downloading,
    Paused,
    Completed,
    Failed,
    Canceled,
    #[default]
    #[serde(other)]
    Pending,
}

/// 跨设备任务的 UI 投影。
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct RemoteTaskDto {
    pub id: String,
    #[serde(default)]
    pub from_device: String,
    #[serde(default)]
    pub to_device: String,
    #[serde(default)]
    pub url: String,
    pub save_dir: Option<String>,
    #[serde(default)]
    pub file_name: String,
    #[serde(default)]
    pub status: RemoteTaskStatus,
    pub total_bytes: Option<i64>,
    #[serde(default)]
    pub downloaded_bytes: i64,
    #[serde(default)]
    pub speed: i64,
    #[serde(default)]
    pub progress: f64,
    pub error: Option<String>,
    #[serde(default)]
    pub created_at: String,
    #[serde(default)]
    pub updated_at: String,
}

/// UI Gateway 运行状态；永远不携带 token 文本。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GatewayStatusDto {
    pub takeover_enabled: bool,
    pub jsonrpc_enabled: bool,
    pub api_enabled: bool,
    pub mcp_enabled: bool,
    pub cors_enabled: bool,
    pub user_token_configured: bool,
}

/// 原子修改 agent 托管的兼容网关开关与用户 token。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct GatewayPatchParams {
    pub takeover_enabled: Option<bool>,
    pub jsonrpc_enabled: Option<bool>,
    pub api_enabled: Option<bool>,
    pub mcp_enabled: Option<bool>,
    pub cors_enabled: Option<bool>,
    /// `Some("")` 清除用户 token；省略则保持。
    pub user_token: Option<String>,
}

/// agent 配置同步状态投影。
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusDto {
    pub enabled: bool,
    pub revision: u64,
    pub dirty_keys: Vec<String>,
    pub last_error: Option<String>,
}

/// agent 自有偏好设置及其原子版本。
#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct AgentPreferencesDto {
    pub revision: u64,
    pub values: BTreeMap<String, Value>,
}

/// 等待官方 UI 确认的外部捕获请求；不包含 cookie 或 header。
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[cfg_attr(feature = "openapi", derive(utoipa::ToSchema))]
#[serde(rename_all = "camelCase")]
pub struct PendingCaptureDto {
    pub transaction_id: String,
    pub url: String,
    #[serde(default)]
    pub file_name: String,
    pub created_at_unix_ms: i64,
}
