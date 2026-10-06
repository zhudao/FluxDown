import FluxDomain
import FluxUI
import SwiftUI

/// X1 BT 文件 / X2 HLS 画质 / X3 插件变体：引擎发起的选择请求（`02-downloads.md` §16）。
///
/// - 请求从 `state.selections` 消失即关闭（含其它设备已答复）。
/// - 倒计时按 `deadlineUnixMs` 与当前时间重算（`CountdownRing`），到 0 以 `defaultChoice` 答复。
/// - X1 / X3 可取消（`.cancelled`，任务保持暂停）；X2 不可取消、不可划走。
/// - 下拉关闭 ≠ 取消：请求仍待处理，由根视图记入 `dismissedSelections`，下载页横幅可重新打开。
struct SelectionRequestSheet: View {
    let requestId: String

    @Environment(AppContainer.self) private var container

    var body: some View {
        SelectionHost(requestId: requestId, container: container)
    }
}

private struct SelectionHost: View {
    let requestId: String
    let container: AppContainer

    @Environment(\.dismiss) private var dismiss
    @State private var resolver: SelectionResolver
    /// 请求消失后的退场动画期间沿用最后一次内容。
    @State private var last: SelectionRequest?

    init(requestId: String, container: AppContainer) {
        self.requestId = requestId
        self.container = container
        _resolver = State(initialValue: SelectionResolver(container: container))
    }

    private var current: SelectionRequest? {
        container.store.state.selections.first { $0.requestId == requestId }
    }

    var body: some View {
        Group {
            if let shown = current ?? last {
                content(shown)
            } else {
                Color.clear
            }
        }
        .onChange(of: current, initial: true) { _, request in
            if let request {
                last = request
            } else {
                // 请求消失而不是我们答复的 → 其它设备已完成选择（或引擎已按默认处理）。
                if !resolver.wasOurs(requestId) {
                    container.toasts.show(text: L("mobileSelectionResolvedElsewhere"), tone: .info, systemImage: "checkmark.circle")
                }
                dismiss()
            }
        }
    }

    @ViewBuilder
    private func content(_ request: SelectionRequest) -> some View {
        let task = container.store.state.task(request.taskId)
        switch request.kind {
        case let .bt(files):
            BtSelectionView(request: request, files: files, task: task, resolver: resolver, close: { dismiss() })
                .id(request.requestId)
        case let .hls(options):
            HlsSelectionView(request: request, options: options, task: task, resolver: resolver, close: { dismiss() })
                .id(request.requestId)
        case let .variant(options):
            VariantSelectionView(request: request, options: options, task: task, resolver: resolver, close: { dismiss() })
                .id(request.requestId)
        }
    }
}

/// 答复成功后关闭 Sheet 的回调（在主 actor 上执行，可跨 Task 传递）。
private typealias SelectionClose = @MainActor @Sendable () -> Void

// MARK: - 答复

/// 经 `HostSession.resolveSelection` 答复；去重、只读拦截与错误 toast 集中在这里。
@MainActor
@Observable
private final class SelectionResolver {
    @ObservationIgnored private unowned let container: AppContainer
    @ObservationIgnored private var inFlight = Set<String>()
    @ObservationIgnored private var ours = Set<String>()

    init(container: AppContainer) {
        self.container = container
    }

    func wasOurs(_ requestId: String) -> Bool { ours.contains(requestId) }

    /// - Parameters:
    ///   - successToast: 答复成功后的提示；nil 不提示。
    ///   - userInitiated: 用户操作才受只读拦截；倒计时自动答复照常尝试。
    func resolve(
        _ request: SelectionRequest,
        _ outcome: SelectionOutcome,
        successToast: String?,
        userInitiated: Bool,
        onSuccess: @escaping SelectionClose
    ) {
        let toasts = container.toasts
        if userInitiated, container.store.state.isReadOnly {
            FluxHaptic.error.play()
            toasts.show(text: L("localServiceDisconnected"), tone: .error, systemImage: "wifi.slash")
            return
        }
        let id = request.requestId
        guard inFlight.insert(id).inserted else { return }
        ours.insert(id) // 先登记：请求消失的事件可能先于本任务恢复到达
        let session = container.session
        Task {
            defer { inFlight.remove(id) }
            do throws(HostError) {
                try await session.resolveSelection(id, outcome: outcome)
                FluxHaptic.success.play()
                if let successToast { toasts.show(text: successToast, tone: .success, systemImage: "checkmark.circle") }
                onSuccess()
            } catch {
                // 请求已不在待处理列表（引擎超时自行按默认项处理 / 其它设备先答复）：不算失败。
                if container.store.state.selections.contains(where: { $0.requestId == id }) {
                    ours.remove(id)
                    FluxHaptic.error.play()
                    toasts.show(text: ErrorText.describe(error), tone: .error)
                }
            }
        }
    }

    /// 取消：任务保持暂停。
    func cancel(_ request: SelectionRequest, onSuccess: @escaping SelectionClose) {
        resolve(request, .cancelled, successToast: L("mobileSelectionCancelled"), userInitiated: true, onSuccess: onSuccess)
    }

    /// 倒计时到期：按默认项答复。
    func applyDefault(_ request: SelectionRequest, onSuccess: @escaping SelectionClose) {
        resolve(request, request.defaultChoice, successToast: L("mobileSelectionAutoApplied"), userInitiated: false, onSuccess: onSuccess)
    }
}

// MARK: - 共用 UI

/// 倒计时基准：首次出现时固定 `total`，环按 deadline 重算（重建视图不会重置）。
private struct SelectionTiming {
    let deadline: Date
    let total: TimeInterval

    init(_ request: SelectionRequest) {
        deadline = Date(timeIntervalSince1970: TimeInterval(request.deadlineUnixMs) / 1000)
        total = max(deadline.timeIntervalSinceNow, 1)
    }
}

/// 任务行：类别图标 + 等宽文件名 + 协议徽标。
private struct SelectionTaskRow: View {
    let task: DownloadTask?

    var body: some View {
        if let task {
            HStack(spacing: 12) {
                KindIcon(kind: task.protocol == .bt ? .torrent : FileKind.from(fileName: task.fileName), size: 36)
                Text(task.fileName)
                    .font(.subheadline.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                ProtocolBadge(text: tag(task.protocol))
            }
            .accessibilityElement(children: .ignore)
            .accessibilityLabel("\(tag(task.protocol)), \(task.fileName)")
        }
    }

    private func tag(_ p: TaskProtocol) -> String { TaskDetailFormat.protocolTag(p) }
}

/// 底部栏：左侧倒计时环 + 文案，右侧主操作。使用系统 `.bottomBar` 工具栏——系统只渲染一层 Liquid Glass，
/// 在 medium（玻璃）/ large（不透明）Sheet 里都不会出现玻璃叠玻璃。
private struct SelectionBar<Primary: View>: ToolbarContent {
    let timing: SelectionTiming
    let onExpire: () -> Void
    @ViewBuilder let primary: Primary

    var body: some ToolbarContent {
        ToolbarItem(placement: .bottomBar) {
            HStack(spacing: 8) {
                CountdownRing(
                    deadline: timing.deadline,
                    total: timing.total,
                    label: { L("selectionAutoDefaultIn", ["seconds": $0]) },
                    onExpire: onExpire
                )
                TimelineView(.periodic(from: .now, by: 1)) { context in
                    let seconds = max(0, Int(timing.deadline.timeIntervalSince(context.date).rounded(.up)))
                    Text(L("selectionAutoDefaultIn", ["seconds": seconds]))
                        .font(.footnote)
                        .monospacedDigit()
                        .lineLimit(2)
                        .foregroundStyle(seconds <= 5 ? Color.fdStatusWarningText : Color.secondary)
                }
                .accessibilityHidden(true)
            }
        }
        ToolbarSpacer(.flexible, placement: .bottomBar)
        ToolbarItem(placement: .bottomBar) { primary }
    }
}

private func hlsLabel(height: Int64, bandwidth: Int64) -> String {
    height > 0 ? "\(height)p" : L("mobileKbps", ["n": bandwidth / 1000])
}

// MARK: - X1 · BT 文件

private struct BtSelectionView: View {
    let request: SelectionRequest
    let files: [BtFile]
    let task: DownloadTask?
    let resolver: SelectionResolver
    let close: SelectionClose

    @Environment(AppContainer.self) private var container
    @State private var timing: SelectionTiming
    @State private var tree: BtFolderNode
    @State private var sizes: [Int32: Int64]
    @State private var initial: Set<Int32>
    @State private var selected: Set<Int32>
    @State private var collapsed: Set<String>
    @State private var showDiscard = false

    init(request: SelectionRequest, files: [BtFile], task: DownloadTask?, resolver: SelectionResolver, close: @escaping SelectionClose) {
        self.request = request
        self.files = files
        self.task = task
        self.resolver = resolver
        self.close = close
        let root = buildBtTree(files)
        var defaults: [Int32] = []
        if case let .bt(indices) = request.defaultChoice { defaults = indices }
        let start = initialBtSelection(files: files, defaultIndices: defaults)
        _timing = State(initialValue: SelectionTiming(request))
        _tree = State(initialValue: root)
        _sizes = State(initialValue: Dictionary(files.map { ($0.index, $0.size) }, uniquingKeysWith: { first, _ in first }))
        _initial = State(initialValue: start)
        _selected = State(initialValue: start)
        _collapsed = State(initialValue: defaultCollapsedFolders(root, fileCount: files.count))
    }

    private var selectedBytes: Int64 { selected.reduce(0) { $0 + (sizes[$1] ?? 0) } }
    private var dirty: Bool { selected != initial }

    var body: some View {
        let rows = flattenBtTree(tree, collapsed: collapsed)
        let sizeText = Format.bytes(selectedBytes).description
        NavigationStack {
            List {
                Section {
                    SelectionTaskRow(task: task)
                } header: {
                    Text(files.count == 1 ? L("btFileSelectDescSingle") : L("btFileSelectDesc", ["count": files.count]))
                        .textCase(nil)
                }
                Section {
                    ForEach(rows, id: \.id) { node in
                        row(node)
                    }
                } header: {
                    toolbarRow(sizeText)
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle(L("btFileSelectTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L("cancel"), systemImage: "xmark") {
                        if dirty { showDiscard = true } else { resolver.cancel(request, onSuccess: close) }
                    }
                }
            }
            .toolbar {
                SelectionBar(timing: timing, onExpire: { resolver.applyDefault(request, onSuccess: close) }) {
                    Button {
                        resolver.resolve(
                            request,
                            .bt(indices: selected.sorted()),
                            successToast: L("mobileDownloadStarted"),
                            userInitiated: true,
                            onSuccess: close
                        )
                    } label: {
                        Text(L("btFileSelectConfirm", ["count": selected.count, "size": sizeText]))
                            .monospacedDigit()
                            .lineLimit(1)
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(selected.isEmpty)
                }
            }
        }
        .confirmationDialog(L("mobileSelectionDiscardTitle"), isPresented: $showDiscard, titleVisibility: .visible) {
            Button(L("mobileDiscard"), role: .destructive) { resolver.cancel(request, onSuccess: close) }
            Button(L("mobileKeepEditing"), role: .cancel) {}
        } message: {
            Text(L("mobileSelectionDiscardMessage"))
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }

    private func toolbarRow(_ sizeText: String) -> some View {
        HStack {
            Button(L("btFileSelectAll")) {
                selected = Set(files.map(\.index))
                FluxHaptic.selection.play()
            }
            Button(L("deselectAll")) {
                selected = []
                FluxHaptic.selection.play()
            }
            Spacer()
            Text(L("selectedCount", ["n": selected.count]) + " · " + sizeText)
                .font(.footnote.monospaced())
                .foregroundStyle(.secondary)
                .lineLimit(1)
        }
        .buttonStyle(.borderless)
        .font(.subheadline)
        .textCase(nil)
    }

    @ViewBuilder
    private func row(_ node: BtNode) -> some View {
        let indent = CGFloat(min(max(node.depth, 0), 4)) * 18
        switch node {
        case let .folder(folder):
            let state = btFolderState(folder, selected: selected)
            let isCollapsed = collapsed.contains(folder.path)
            HStack(spacing: 8) {
                Button(isCollapsed ? L("mobileExpand") : L("mobileCollapse"), systemImage: "chevron.down") {
                    if isCollapsed { collapsed.remove(folder.path) } else { collapsed.insert(folder.path) }
                }
                .labelStyle(.iconOnly)
                .font(.footnote.weight(.semibold))
                .foregroundStyle(.secondary)
                .rotationEffect(.degrees(isCollapsed ? -90 : 0))
                .fluxAnimation(.snappy, value: isCollapsed)
                .buttonStyle(.borderless)
                .frame(minWidth: 32, minHeight: 44)
                Button {
                    if state == .on { selected.subtract(folder.indices) } else { selected.formUnion(folder.indices) }
                    FluxHaptic.selection.play()
                } label: {
                    HStack(spacing: 10) {
                        checkbox(state)
                        Image(systemName: "folder").foregroundStyle(.secondary)
                        Text(folder.name).lineLimit(1).truncationMode(.middle)
                        Spacer(minLength: 8)
                        Text(Format.bytes(folder.size).description)
                            .font(.footnote.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                    .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
                .accessibilityElement(children: .ignore)
                .accessibilityLabel(folder.name)
                .accessibilityValue(Format.bytes(folder.size).description)
                .accessibilityAddTraits(state == .on ? [.isSelected] : [])
            }
            .padding(.leading, indent)
        case let .file(name, _, file):
            let on = selected.contains(file.index)
            Button {
                if on { selected.remove(file.index) } else { selected.insert(file.index) }
                FluxHaptic.selection.play()
            } label: {
                HStack(spacing: 10) {
                    checkbox(on ? .on : .off)
                    Image(systemName: FileKind.from(fileName: name).symbolName).foregroundStyle(.secondary)
                    Text(name).lineLimit(1).truncationMode(.middle)
                    Spacer(minLength: 8)
                    Text(Format.bytes(file.size).description)
                        .font(.footnote.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)
            .padding(.leading, indent + 40)
            .accessibilityElement(children: .ignore)
            .accessibilityLabel(name)
            .accessibilityValue(Format.bytes(file.size).description)
            .accessibilityAddTraits(on ? [.isSelected] : [])
        }
    }

    private func checkbox(_ state: BtCheckState) -> some View {
        Image(systemName: {
            switch state {
            case .off: "circle"
            case .on: "checkmark.circle.fill"
            case .mixed: "minus.circle.fill"
            }
        }())
        .font(.title3)
        .foregroundStyle(state == .off ? Color.secondary : Color.accentColor)
        .accessibilityHidden(true)
    }
}

// MARK: - X2 · HLS 画质

private struct HlsSelectionView: View {
    let request: SelectionRequest
    let task: DownloadTask?
    let resolver: SelectionResolver
    let close: SelectionClose

    @State private var timing: SelectionTiming
    @State private var sorted: [HlsOption]
    @State private var selected: Int32?

    init(request: SelectionRequest, options: [HlsOption], task: DownloadTask?, resolver: SelectionResolver, close: @escaping SelectionClose) {
        self.request = request
        self.task = task
        self.resolver = resolver
        self.close = close
        let list = options.sorted { $0.bandwidth > $1.bandwidth }
        var defaultIndex: Int32?
        if case let .hls(index) = request.defaultChoice { defaultIndex = index }
        _timing = State(initialValue: SelectionTiming(request))
        _sorted = State(initialValue: list)
        _selected = State(initialValue: list.first { $0.index == defaultIndex }?.index ?? list.first?.index)
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    SelectionTaskRow(task: task)
                } header: {
                    Text(L("hlsQualityDesc")).textCase(nil)
                }
                Section {
                    ForEach(Array(sorted.enumerated()), id: \.element.index) { offset, option in
                        Button {
                            selected = option.index
                            FluxHaptic.selection.play()
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(hlsLabel(height: option.height, bandwidth: option.bandwidth))
                                    if let subtitle = subtitle(option) {
                                        Text(subtitle).font(.footnote).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer(minLength: 8)
                                if offset == 0 {
                                    Text(L("mobileQualityBest")).font(.footnote).foregroundStyle(.secondary)
                                }
                                if option.index == selected {
                                    Image(systemName: "checkmark").foregroundStyle(.tint).fontWeight(.semibold)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(option.index == selected ? [.isSelected] : [])
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle(L("hlsQualityTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                SelectionBar(timing: timing, onExpire: { resolver.applyDefault(request, onSuccess: close) }) {
                    Button {
                        guard let pick = sorted.first(where: { $0.index == selected }) else { return }
                        resolver.resolve(
                            request,
                            .hls(index: pick.index),
                            successToast: L("mobileSelectionPicked", ["label": hlsLabel(height: pick.height, bandwidth: pick.bandwidth)]),
                            userInitiated: true,
                            onSuccess: close
                        )
                    } label: {
                        Text(L("confirm"))
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(selected == nil)
                }
            }
        }
        .interactiveDismissDisabled()
        .presentationDetents([.medium, .large])
    }

    private func subtitle(_ o: HlsOption) -> String? {
        var parts: [String] = []
        if o.width > 0, o.height > 0 { parts.append("\(o.width)×\(o.height)") }
        if o.bandwidth > 0 { parts.append(L("mobileKbps", ["n": o.bandwidth / 1000])) }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}

// MARK: - X3 · 插件变体

private struct VariantSelectionView: View {
    let request: SelectionRequest
    let options: [VariantOption]
    let task: DownloadTask?
    let resolver: SelectionResolver
    let close: SelectionClose

    @State private var timing: SelectionTiming
    @State private var selected: Int32?

    init(request: SelectionRequest, options: [VariantOption], task: DownloadTask?, resolver: SelectionResolver, close: @escaping SelectionClose) {
        self.request = request
        self.options = options
        self.task = task
        self.resolver = resolver
        self.close = close
        var defaultIndex: Int32?
        if case let .variant(index) = request.defaultChoice { defaultIndex = index }
        _timing = State(initialValue: SelectionTiming(request))
        _selected = State(initialValue: options.first { $0.index == defaultIndex }?.index ?? options.first?.index)
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    SelectionTaskRow(task: task)
                } header: {
                    Text(L("resolveVariantDesc")).textCase(nil)
                }
                Section {
                    ForEach(options, id: \.index) { option in
                        Button {
                            selected = option.index
                            FluxHaptic.selection.play()
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(option.label)
                                    if let subtitle = subtitle(option) {
                                        Text(subtitle).font(.footnote).foregroundStyle(.secondary)
                                    }
                                }
                                Spacer(minLength: 8)
                                if option.totalBytes > 0 {
                                    Text(Format.bytes(option.totalBytes).description)
                                        .font(.footnote.monospaced())
                                        .foregroundStyle(.secondary)
                                }
                                if option.index == selected {
                                    Image(systemName: "checkmark").foregroundStyle(.tint).fontWeight(.semibold)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .accessibilityAddTraits(option.index == selected ? [.isSelected] : [])
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle(L("resolveVariantTitle"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L("cancel"), systemImage: "xmark") { resolver.cancel(request, onSuccess: close) }
                }
            }
            .toolbar {
                SelectionBar(timing: timing, onExpire: { resolver.applyDefault(request, onSuccess: close) }) {
                    Button {
                        guard let pick = options.first(where: { $0.index == selected }) else { return }
                        resolver.resolve(
                            request,
                            .variant(index: pick.index),
                            successToast: L("mobileSelectionPicked", ["label": pick.label]),
                            userInitiated: true,
                            onSuccess: close
                        )
                    } label: {
                        Text(L("confirm"))
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(selected == nil)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func subtitle(_ o: VariantOption) -> String? {
        var parts: [String] = []
        if !o.container.isEmpty { parts.append(o.container) }
        if o.width > 0, o.height > 0 { parts.append("\(o.width)×\(o.height)") }
        return parts.isEmpty ? nil : parts.joined(separator: " · ")
    }
}
