//! FluxDown 本机服务与客户端共享的传输无关协议类型。
//!
//! 本 crate 只定义 wire 契约；不得依赖下载引擎、网络运行时、数据库或 UI。

pub mod agent;
pub mod daemon;
pub mod error;
pub mod event;
pub mod method;
pub mod rpc;
pub mod settings;

pub use agent::{
    AgentLoginResult, AgentPreferencesDto, AgentSessionDto, AuthVerificationDto, CloudDevice,
    CloudOrder, CloudPlan, CloudPlanCampaign, CloudPlanCampaignStage, CloudProfile,
    CloudReferralCode, CloudReferralCodesResult, CloudReferralRecord, CloudReferralRecordsResult,
    CloudReferralRule, CloudReferralSummary, CloudReferralValidateResult, CloudUser,
    CloudUserStatus, Entitlements, GatewayPatchParams, GatewayStatusDto, OriginIdCheckResult,
    PendingCaptureDto, RemoteTaskDto, RemoteTaskStatus, SyncStatusDto,
};
pub use daemon::{
    ApiInfo, BtFileDto, CdnConfigApplyParams, CdnNodeDto, CdnReportAckParams, CdnReportLeaseDto,
    ComponentFfmpegStatus, ComponentInstallParams, ComponentKind, ComponentParams,
    ComponentStatusDto, ComponentVersions, ComponentYtdlpStatus, CreateGroupRequest,
    CreateGroupResponse, CreateQueueRequest, CreateTaskRequest, CreatedTask, DaemonConfigPatch,
    DaemonConfigSnapshot, DaemonCreateTaskParams, DaemonRuntimeStatsDto, DownloadRequest,
    Ed2kServerSubRefreshResponse, FileMissingUpdateDto, FsEntry, FsListResponse,
    GatewayMigrationExport, GroupDto, GroupItemRequest, HlsQualityOptionDto, InstallFfmpegRequest,
    InstallPluginDevRequest, InstalledPlugin, LATER_QUEUE_ID, LinkAuth, LinkCodeResponse,
    LinkDeviceInfo, LinkDeviceTaskRequest, LinkDevicesResponse, LinkDiscoveredPeer,
    LinkDiscoveredResponse, LinkDiscoveryRequest, LinkMigrationExport, LinkOkResponse,
    LinkPairApproveRequest, LinkPairBeginRequest, LinkPairBeginResponse, LinkPairConfirmOutcome,
    LinkPairConfirmRequest, LinkPairFinishRequest, LinkPairFinishResponse, LinkPairHelloRequest,
    LinkPairHelloResponse, LinkPingInfo, LinkProbeRequest, LinkTaskRequest, LogFileDto,
    LogsResponse, MAIN_QUEUE_ID, MarketEntryDto, MarketInstallRequest, MigrationAckParams,
    MoveQueueRequest, PluginDto, PreviewItemDto, PreviewVariantDto, ProxyTestRequest,
    ProxyTestResponse, QueueDto, QueuePositionDto, QueueScheduleRequest, RenameTaskRequest,
    ReorderQueueRequest, RequestBody, ResolvePreviewRequest, ResolvePreviewResponse,
    ResolveVariantOptionDto, ResultMessage, RssItemActionRequest, RssItemDto, RssSourceDto,
    RssValidateRequest, RssValidateResponse, SegmentDetailDto, SelectionKind, SelectionOutcome,
    SelectionRequestDto, SelectionResolutionDto, SetPluginEnabledRequest, SettingFieldDto,
    SettingOptionDto, SetupRequest, SetupStatusResponse, StatsResponse, TaskDto, TokenResponse,
    TrackerSubRefreshResponse, UpdateQueueRequest, WebhookDeliveriesResponse, WebhookDeliveryDto,
    WebhookPresetDto, WebhookSimulateResponse, WebhookTestRequest, WebhookTestResponse,
    WsClientMsg, WsServerMsg,
};
pub use error::{
    APPLICATION_ERROR_CODE, ApplicationErrorCode, INTERNAL_ERROR_CODE, INVALID_PARAMS_CODE,
    INVALID_REQUEST_CODE, METHOD_NOT_FOUND_CODE, PARSE_ERROR_CODE, RpcErrorData, RpcErrorObject,
};
pub use event::{
    AgentEvent, AgentSnapshot, DaemonEvent, DaemonSnapshot, EventFrame, ServiceEvent, Snapshot,
    SnapshotBody,
};
pub use rpc::{
    ClientHello, JSONRPC_VERSION, MIN_PROTOCOL_VERSION, PROTOCOL_VERSION, RequestId,
    RpcFailureResponse, RpcIncoming, RpcNotification, RpcRequest, RpcResponse, RpcSuccessResponse,
    ServiceHello, ServiceRole, negotiate_protocol, validate_first_request,
};
pub use settings::{
    SYNC_SETTING_SPECS, SettingOwner, SettingSpec, SettingValueKind, daemon_config_to_value,
    setting_spec, setting_value_kind, validate_value, value_to_daemon_config,
};
