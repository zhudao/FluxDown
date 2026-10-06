import Foundation

// 云端受信任设备（`agent.device.*`）。镜像 `native/protocol/src/agent.rs::CloudDevice` / `PathStyle`，
// 规则镜像 Web `deviceList.ts`、`pages/downloads/dialogs/target.ts` 与 `crates/downloads/src/model/dispatch.rs`。
//
// 与 `Model/Models.swift` 里的 `CloudDevice`（快照里的只读子集，没有云端行 id）不同：`CloudDeviceRecord` 是协议完整形状，
// `rename` / `delete` 需要它的 `id`（不是 `deviceId`）。

/// 设备本地文件路径的书写风格，决定远程下发时保存目录的合法形态。
public enum PathStyle: WireStringEnum {
    /// `C:\dir` / `\\server\share`。
    case windows
    /// `/dir`。
    case posix
    /// 对端发送了本端不认识的风格。
    case unknown(String)

    public init(wire: String) {
        switch wire {
        case "windows": self = .windows
        case "posix": self = .posix
        default: self = .unknown(wire)
        }
    }

    public var wire: String {
        switch self {
        case .windows: "windows"
        case .posix: "posix"
        case let .unknown(raw): raw
        }
    }

    /// 按设备平台名推断（`windows` / `macos` / `linux` / `android` / `ios` …）；未知平台 nil。
    public static func from(platform: String) -> PathStyle? {
        switch platform.trimmingCharacters(in: .whitespacesAndNewlines).lowercased() {
        case "windows", "win32": .windows
        case "macos", "darwin", "linux", "android", "ios", "freebsd", "openbsd", "netbsd": .posix
        default: nil
        }
    }

    /// `path` 是否为该风格下的绝对路径（`unknown` 恒 false）。
    public func isAbsolute(_ path: String) -> Bool {
        let path = path.trimmingCharacters(in: .whitespacesAndNewlines)
        switch self {
        case .windows:
            let bytes = Array(path.utf8)
            let isLetter: (UInt8) -> Bool = { ($0 >= 65 && $0 <= 90) || ($0 >= 97 && $0 <= 122) }
            let drive = bytes.count >= 3 && isLetter(bytes[0]) && bytes[1] == UInt8(ascii: ":")
                && (bytes[2] == UInt8(ascii: "\\") || bytes[2] == UInt8(ascii: "/"))
            return drive || path.hasPrefix("\\\\")
        case .posix:
            return path.hasPrefix("/")
        case .unknown:
            return false
        }
    }

    /// 校验失败提示里的路径示例。
    public static func example(_ style: PathStyle?) -> String {
        if case .windows? = style { return "D:\\Downloads" }
        return "/home/user/Downloads"
    }

    /// 设备自报的路径风格；缺失 / 不认识时按平台推断。
    public static func effective(reported: PathStyle?, platform: String?) -> PathStyle? {
        switch reported {
        case .windows?, .posix?: return reported
        case .unknown?, nil: return platform.flatMap(from(platform:))
        }
    }
}

/// `agent.device.list` 元素 / `agent.session.device`：受信任设备公开投影（`CloudDevice`）。
public struct CloudDeviceRecord: Sendable, Hashable, Identifiable, Codable {
    /// 云端行 id（`rename` / `delete` 用它；不是 `deviceId`）。
    public var id: String
    /// 设备 id（`agent.remote.dispatch.toDevice` 用它）。
    public var deviceId: String
    public var name: String
    public var platform: String?
    public var createdAt: String
    public var lastSeenAt: String
    public var lastIp: String?
    public var appVersion: String?
    public var isOnline: Bool
    public var isCurrent: Bool
    /// 设备自报的默认下载目录（目标设备本地路径）。
    public var defaultSaveDir: String?
    public var pathStyle: PathStyle?

    public init(
        id: String, deviceId: String, name: String = "", platform: String? = nil, createdAt: String = "",
        lastSeenAt: String = "", lastIp: String? = nil, appVersion: String? = nil, isOnline: Bool = false,
        isCurrent: Bool = false, defaultSaveDir: String? = nil, pathStyle: PathStyle? = nil
    ) {
        self.id = id
        self.deviceId = deviceId
        self.name = name
        self.platform = platform
        self.createdAt = createdAt
        self.lastSeenAt = lastSeenAt
        self.lastIp = lastIp
        self.appVersion = appVersion
        self.isOnline = isOnline
        self.isCurrent = isCurrent
        self.defaultSaveDir = defaultSaveDir
        self.pathStyle = pathStyle
    }

    public init(from decoder: any Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        id = try c.decode(String.self, forKey: .id)
        deviceId = try c.decode(String.self, forKey: .deviceId)
        name = try c.decodeIfPresent(String.self, forKey: .name) ?? ""
        platform = try c.decodeIfPresent(String.self, forKey: .platform)
        createdAt = try c.decodeIfPresent(String.self, forKey: .createdAt) ?? ""
        lastSeenAt = try c.decodeIfPresent(String.self, forKey: .lastSeenAt) ?? ""
        lastIp = try c.decodeIfPresent(String.self, forKey: .lastIp)
        appVersion = try c.decodeIfPresent(String.self, forKey: .appVersion)
        isOnline = try c.decodeIfPresent(Bool.self, forKey: .isOnline) ?? false
        isCurrent = try c.decodeIfPresent(Bool.self, forKey: .isCurrent) ?? false
        defaultSaveDir = try c.decodeIfPresent(String.self, forKey: .defaultSaveDir)
        pathStyle = try c.decodeIfPresent(PathStyle.self, forKey: .pathStyle)
    }

    /// 有效路径风格（自报 → 平台推断）。
    public var effectivePathStyle: PathStyle? { PathStyle.effective(reported: pathStyle, platform: platform) }
}

/// `agent.device.list` 结果（同时刷新快照里的 `cloudDevices`）。
public struct CloudDeviceList: Sendable, Hashable, Codable {
    public var devices: [CloudDeviceRecord]

    public init(devices: [CloudDeviceRecord]) { self.devices = devices }
}

public struct DeviceRenameParams: Sendable, Hashable, Codable {
    /// `CloudDeviceRecord.id`（不是 deviceId）。
    public var id: String
    /// 1–64 字符。
    public var name: String

    public init(id: String, name: String) {
        self.id = id
        self.name = name
    }
}

public struct DeviceIdParams: Sendable, Hashable, Codable {
    public var id: String

    public init(id: String) { self.id = id }
}

/// `agent.remote.reconnect` 结果：`accepted` 不表示已连上。
public struct RemoteReconnectResult: Sendable, Hashable, Codable {
    public var accepted: Bool

    public init(accepted: Bool) { self.accepted = accepted }
}

// MARK: - 纯规则

/// 设备列表 / 下发目标的纯规则。
public enum DeviceRules {
    /// 云端设备排序（`crates/account/src/device_list.rs::sorted`）：本机在前 → 在线（presence 已知时）→ 名称（不区分大小写）；
    /// 稳定（同名保持原序）。
    public static func sorted(_ devices: [CloudDeviceRecord], presenceKnown: Bool) -> [CloudDeviceRecord] {
        devices.enumerated().sorted { lhs, rhs in
            let a = lhs.element, b = rhs.element
            if a.isCurrent != b.isCurrent { return a.isCurrent }
            let aOnline = presenceKnown && a.isOnline, bOnline = presenceKnown && b.isOnline
            if aOnline != bOnline { return aOnline }
            switch a.name.compare(b.name, options: .caseInsensitive) {
            case .orderedAscending: return true
            case .orderedDescending: return false
            case .orderedSame: return lhs.offset < rhs.offset
            }
        }.map(\.element)
    }

    /// 「其他设备」：账号设备去掉本机。
    public static func others(_ devices: [CloudDeviceRecord]) -> [CloudDeviceRecord] {
        devices.filter { !$0.isCurrent }
    }

    /// 本机在 FluxCloud 的 deviceId：优先会话，其次设备列表里的 `isCurrent`。
    public static func currentDeviceId(session: AgentSessionDto?, devices: [CloudDeviceRecord]) -> String? {
        session?.device.deviceId ?? devices.first(where: \.isCurrent)?.deviceId
    }

    /// 搜索（名称 / 平台，不区分大小写与变音）；空查询返回全部。
    public static func filter(_ devices: [CloudDeviceRecord], query: String) -> [CloudDeviceRecord] {
        let needle = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !needle.isEmpty else { return devices }
        let options: String.CompareOptions = [.caseInsensitive, .diacriticInsensitive]
        return devices.filter {
            $0.name.range(of: needle, options: options) != nil || ($0.platform?.range(of: needle, options: options) != nil)
        }
    }

    /// 远端保存目录输入的校验结果（`dispatch.rs::check_remote_save_dir`）。
    public enum SaveDirCheck: Sendable, Hashable {
        /// 输入为空：提交时不带 `saveDir`，目标设备用它自己的默认目录。
        case useDefault
        /// 合法目录（已去首尾空白）。
        case explicit(String)
        /// 不是目标路径风格下的绝对路径。
        case invalid
    }

    /// 风格未知（Web 端 / 新平台）时无法判断，交给目标设备回退默认目录，只要求非空即放行。
    public static func checkSaveDir(_ input: String, style: PathStyle?) -> SaveDirCheck {
        let dir = input.trimmingCharacters(in: .whitespacesAndNewlines)
        if dir.isEmpty { return .useDefault }
        if let style {
            if case .unknown = style { return .explicit(dir) }
            return style.isAbsolute(dir) ? .explicit(dir) : .invalid
        }
        return .explicit(dir)
    }
}
