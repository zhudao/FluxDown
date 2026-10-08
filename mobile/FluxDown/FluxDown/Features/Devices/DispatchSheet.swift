import FluxDomain
import FluxUI
import Foundation
import Observation
import SwiftUI

/// V3a 下发目标：云端受信任设备（经 FluxCloud）或局域网已配对设备（直连）。持有下发时刻的设备快照。
enum DispatchTarget {
    case cloud(CloudDeviceRecord, presenceKnown: Bool)
    case link(LinkDeviceInfo)

    var name: String {
        switch self {
        case let .cloud(record, _): record.name
        case let .link(info): info.name
        }
    }

    /// 保存目录按目标设备的路径风格校验（自报 → 平台推断；未知则只要求非空）。
    var pathStyle: PathStyle? {
        switch self {
        case let .cloud(record, _): record.effectivePathStyle
        case let .link(info): info.effectivePathStyle
        }
    }

    var defaultSaveDir: String? {
        let dir: String? = switch self {
        case let .cloud(record, _): record.defaultSaveDir
        case let .link(info): info.defaultSaveDir
        }
        guard let dir = dir?.trimmingCharacters(in: .whitespacesAndNewlines), !dir.isEmpty else { return nil }
        return dir
    }

    /// 已知离线：云端 presence 已知且设备离线（云端排队，上线后执行）/ 局域网设备探测为离线（直连送不到）。
    var isOffline: Bool {
        switch self {
        case let .cloud(record, presenceKnown): presenceKnown && !record.isOnline
        case let .link(info): !info.online
        }
    }

    var isCloud: Bool {
        if case .cloud = self { true } else { false }
    }
}

/// 一次下发（可能多条链接）的结果汇总（对齐 GPUI `DispatchSummary`）：成功数、失败数、第一条错误。
struct DispatchSummary {
    var successes = 0
    var failures = 0
    var firstError: HostError?
    var failedEntries: [UrlEntry] = []
}

/// V3a 下发表单的状态与提交：每条链接一次 dispatch（顺序执行，避免一次性压垮对端），全部结束后汇总。
@MainActor
@Observable
final class DispatchModel {
    var urlText = ""
    var fileName = ""
    var saveDir = ""
    private(set) var sending = false
    private(set) var done = 0
    private(set) var total = 0
    /// 内联错误（全部失败 / 部分失败时保留失败的链接供重试）。
    private(set) var failure: String?

    let target: DispatchTarget
    @ObservationIgnored private unowned let container: AppContainer

    init(target: DispatchTarget, container: AppContainer) {
        self.target = target
        self.container = container
    }

    var entries: [UrlEntry] { parseEntries(urlText).dedupe() }

    var saveDirCheck: DeviceRules.SaveDirCheck {
        DeviceRules.checkSaveDir(saveDir, style: target.pathStyle)
    }

    /// 提交；返回是否全部成功（成功时调用方关闭表单）。
    func send() async -> Bool {
        guard !sending else { return false }
        let entries = entries
        guard !entries.isEmpty else { return false }
        let dir: String?
        switch saveDirCheck {
        case .invalid: return false
        case .useDefault: dir = nil
        case let .explicit(path): dir = path
        }
        sending = true
        failure = nil
        done = 0
        total = entries.count
        defer { sending = false }

        let typedName = fileName.trimmingCharacters(in: .whitespacesAndNewlines)
        let single = entries.count == 1
        var summary = DispatchSummary()
        for entry in entries {
            let name = single && !typedName.isEmpty ? typedName : entry.fileName
            do throws(HostError) {
                try await dispatch(url: entry.url, fileName: name.isEmpty ? nil : name, saveDir: dir)
                summary.successes += 1
            } catch {
                summary.failures += 1
                summary.failedEntries.append(entry)
                if summary.firstError == nil { summary.firstError = error }
            }
            done += 1
        }
        return report(summary)
    }

    private func dispatch(url: String, fileName: String?, saveDir: String?) async throws(HostError) {
        switch target {
        case let .cloud(record, _):
            _ = try await container.agent.remoteDispatch(
                RemoteDispatchParams(toDevice: record.deviceId, url: url, fileName: fileName, saveDir: saveDir)
            )
        case let .link(info):
            _ = try await container.agent.linkDispatch(
                LinkDispatchParams(fingerprint: info.fingerprint, url: url, fileName: fileName, saveDir: saveDir)
            )
        }
    }

    private func report(_ summary: DispatchSummary) -> Bool {
        let device = target.name
        if summary.failures == 0 {
            let text: String
            if target.isCloud, target.isOffline {
                text = L("downloadToDispatchedOffline", ["count": summary.successes, "device": device])
            } else if summary.successes == 1 {
                text = L("dispatchedToDevice", ["device": device])
            } else {
                text = L("downloadToDispatched", ["count": summary.successes, "device": device])
            }
            container.toasts.show(text: text, tone: .success)
            FluxHaptic.success.play()
            return true
        }
        let reason = summary.firstError.map { AccountText.error($0, context: target.isCloud ? .general : .pairing) }
            ?? L("dispatchFailed")
        // 只保留失败的链接，成功的不会被重复下发。
        urlText = summary.failedEntries.map { $0.toText() }.joined(separator: "\n")
        if summary.successes > 0 {
            let partial = L(
                "downloadToPartial",
                ["ok": summary.successes, "failed": summary.failures, "device": device]
            )
            container.toasts.show(text: partial, tone: .warning)
            failure = partial + "\n" + reason
            FluxHaptic.warning.play()
        } else {
            failure = reason
            FluxHaptic.error.play()
        }
        return false
    }
}

/// V3a 下发下载 sheet：多行链接（aria2 风格，与新建下载同一解析）、可选文件名（单条链接时）、可选保存目录
/// （按目标设备路径风格校验，留空 = 目标设备默认目录）。
struct DispatchSheet: View {
    @Environment(AppContainer.self) private var container
    let target: DispatchTarget

    var body: some View {
        DispatchForm(target: target, container: container)
    }
}

private struct DispatchForm: View {
    @Environment(AppContainer.self) private var container
    @Environment(\.dismiss) private var dismiss
    @State private var model: DispatchModel
    @FocusState private var linksFocused: Bool

    init(target: DispatchTarget, container: AppContainer) {
        _model = State(initialValue: DispatchModel(target: target, container: container))
    }

    var body: some View {
        @Bindable var model = model
        let entries = model.entries
        let readOnly = container.store.state.isReadOnly
        let target = model.target
        let saveDirInvalid: Bool = if case .invalid = model.saveDirCheck { true } else { false }
        let canSend = !entries.isEmpty && !saveDirInvalid && !readOnly && !model.sending
        NavigationStack {
            Form {
                if readOnly {
                    Section {
                        Banner(text: L("localServiceDisconnected"), tone: .warning, systemImage: FluxSymbol.offline, slim: true)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets())
                    }
                }
                if target.isOffline {
                    Section {
                        Banner(
                            text: L(target.isCloud ? "downloadToOfflineHint" : "errReasonPeerOffline"),
                            tone: .warning,
                            systemImage: "moon.zzz.fill",
                            slim: true
                        )
                        .listRowBackground(Color.clear)
                        .listRowInsets(EdgeInsets())
                    }
                }
                linksSection(model: $model, entries: entries)
                Section {
                    TextField(L("filenameOptional"), text: $model.fileName)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                        .disabled(entries.count > 1)
                } footer: {
                    if entries.count > 1 { Text(L("mobileDispatchFileNameSingleOnly")) }
                }
                saveDirSection(model: $model, invalid: saveDirInvalid)
                if model.sending {
                    Section {
                        HStack(spacing: 12) {
                            ProgressView()
                            Text(L("mobileDispatchSending", ["done": model.done, "total": model.total]))
                                .monospacedDigit()
                        }
                        .accessibilityElement(children: .combine)
                    }
                }
                if let failure = model.failure {
                    Section {
                        Banner(text: failure, tone: .error, systemImage: FluxSymbol.failure, slim: true)
                            .listRowBackground(Color.clear)
                            .listRowInsets(EdgeInsets())
                    }
                }
            }
            .scrollDismissesKeyboard(.interactively)
            .navigationTitle(L("mobileDispatchTitle", ["device": target.name]))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L("cancel"), role: .cancel) { dismiss() }
                        .disabled(model.sending)
                }
                ToolbarItem(placement: .confirmationAction) {
                    if model.sending {
                        ProgressView().accessibilityLabel(L("mobileDispatchSend"))
                    } else {
                        Button(L("mobileDispatchSend")) {
                            linksFocused = false
                            Task {
                                if await model.send() { dismiss() }
                            }
                        }
                        .disabled(!canSend)
                    }
                }
            }
            .interactiveDismissDisabled(model.sending)
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
        .linkPairingPrompts()
    }

    // MARK: 链接

    private func linksSection(model: Bindable<DispatchModel>, entries: [UrlEntry]) -> some View {
        let text = model.wrappedValue.urlText
        let hasText = !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let noValid = hasText && entries.isEmpty
        return Section {
            TextEditor(text: model.urlText)
                .font(.callout.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
                .focused($linksFocused)
                .disabled(model.wrappedValue.sending)
                .frame(minHeight: 110)
                .overlay(alignment: .topLeading) {
                    if text.isEmpty {
                        Text(L("batchUrlPlaceholder"))
                            .font(.callout.monospaced())
                            .foregroundStyle(.tertiary)
                            .padding(.top, 8)
                            .padding(.leading, 5)
                            .allowsHitTesting(false)
                            .accessibilityHidden(true)
                    }
                }
                .accessibilityLabel(L("downloadUrl"))
            if noValid {
                Label(L("newDownloadNoValidUrl"), systemImage: FluxSymbol.failure)
                    .font(.footnote)
                    .foregroundStyle(Color.fdStatusFailedText)
            }
        } header: {
            HStack {
                Text(L("downloadUrl"))
                Spacer()
                if !entries.isEmpty {
                    Text(L("urlCount", ["count": entries.count]))
                        .monospacedDigit()
                        .accessibilityAddTraits(.updatesFrequently)
                }
            }
        } footer: {
            Text(L("mobileDispatchUrlsHint"))
        }
    }

    // MARK: 保存目录

    private func saveDirSection(model: Bindable<DispatchModel>, invalid: Bool) -> some View {
        let target = model.wrappedValue.target
        let placeholder = target.defaultSaveDir.map { L("downloadToRemoteDirDefault", ["dir": $0]) }
            ?? L("downloadToRemoteDirUseDefault")
        return Section {
            TextField(L("saveDir"), text: model.saveDir, prompt: Text(placeholder))
                .font(.body.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
                .disabled(model.wrappedValue.sending)
        } header: {
            Text(L("saveDir"))
        } footer: {
            VStack(alignment: .leading, spacing: 4) {
                Text(L("downloadToRemoteDirHint"))
                if invalid {
                    Label(
                        L("downloadToPathInvalid", ["example": PathStyle.example(target.pathStyle)]),
                        systemImage: FluxSymbol.failure
                    )
                    .foregroundStyle(Color.fdStatusFailedText)
                }
            }
        }
    }
}
