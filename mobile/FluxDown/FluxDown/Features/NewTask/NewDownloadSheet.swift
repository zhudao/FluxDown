import FluxDomain
import FluxUI
import SwiftUI
import UniformTypeIdentifiers

/// N1 · 新建下载（`02-downloads.md` §11）+ N2 高级选项（同一 `NavigationStack` 内推入）。
///
/// - 系统 `Form` 承载内容（不上玻璃）；玻璃只在底部浮层操作栏（§2.1 / §2.4）。
/// - 粘贴用系统 `PasteButton`（不主动读剪贴板，避免「允许粘贴」弹窗）。
/// - 表单每次打开新建一份，默认值来自主机配置。
struct NewDownloadSheet: View {
    let prefill: String

    @Environment(AppContainer.self) private var container
    @Environment(\.dismiss) private var dismiss
    @State private var form: NewDownloadForm?

    var body: some View {
        Group {
            if let form {
                NewDownloadContent(form: form, close: { dismiss() })
            } else {
                Color.clear
            }
        }
        .task {
            if form == nil { form = NewDownloadForm(prefill: prefill, state: container.store.state) }
        }
        // 关闭 Sheet 即取消进行中的清单预解析（在途 RPC 的晚到结果被忽略）。
        .onDisappear { form?.cancelProbe() }
    }
}

private struct NewDownloadContent: View {
    @Bindable var form: NewDownloadForm
    let close: () -> Void

    @Environment(AppContainer.self) private var container
    @Environment(\.dynamicTypeSize) private var dynamicTypeSize
    @Environment(\.accessibilityReduceMotion) private var reduceMotion
    @State private var showDiscard = false
    @State private var showImporter = false
    @State private var importerKind: ImporterKind = .text
    @State private var importingTorrent = false
    @State private var dropTargeted = false
    @State private var path: [NewDownloadRoute] = []
    @FocusState private var linksFocused: Bool

    private static let probeId = "manifestProbe"

    private var state: HostState { container.store.state }

    var body: some View {
        let entries = form.entries
        NavigationStack(path: $path) {
            ScrollViewReader { proxy in
                Form {
                    if let probe = form.probe { probeSection(probe) }
                    linksSection(entries)
                    if !entries.isEmpty { previewSection(entries) }
                    destinationSection(entries)
                    optionsSection(entries)
                    advancedSection(entries)
                }
                .fluxAnimation(.smooth, value: form.probe != nil)
                .scrollDismissesKeyboard(.interactively)
                .onChange(of: form.probe?.id) { _, id in
                    guard id != nil else { return }
                    withFluxAnimation(.smooth, reduceMotion: reduceMotion) {
                        proxy.scrollTo(Self.probeId, anchor: .top)
                    }
                }
            }
            .navigationTitle(L("newDownload"))
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button(L("cancel"), systemImage: "xmark", action: requestClose)
                        .disabled(form.submitting)
                }
                ToolbarItemGroup(placement: .keyboard) {
                    Spacer()
                    Button(L("confirm")) { linksFocused = false }
                }
            }
            .toolbar { bottomBar(entries) }
            .navigationDestination(for: NewDownloadRoute.self) { route in
                switch route {
                case .advanced:
                    AdvancedOptionsView(form: form)
                case let .manifest(source, base):
                    ManifestSelectView(source: source, base: base, close: close)
                }
            }
        }
        .onDrop(of: [.fileURL], isTargeted: $dropTargeted) { providers in
            handleDrop(providers)
        }
        .overlay {
            if dropTargeted {
                RoundedRectangle(cornerRadius: 28, style: .continuous)
                    .strokeBorder(.tint, style: StrokeStyle(lineWidth: 3, dash: [10, 6]))
                    .padding(8)
                    .allowsHitTesting(false)
                    .accessibilityHidden(true)
            }
        }
        .fluxAnimation(.snappy, value: dropTargeted)
        .interactiveDismissDisabled(form.isDirty || form.submitting)
        .confirmationDialog(L("mobileDiscardTitle"), isPresented: $showDiscard, titleVisibility: .visible) {
            Button(L("mobileDiscard"), role: .destructive, action: close)
            Button(L("mobileKeepEditing"), role: .cancel) {}
        } message: {
            Text(L("mobileDiscardMessage"))
        }
        .fileImporter(
            isPresented: $showImporter,
            allowedContentTypes: importerKind == .torrent ? [TorrentImport.contentType] : [.plainText, .text, .data],
            allowsMultipleSelection: true
        ) { result in
            switch importerKind {
            case .text: importFiles(result)
            case .torrent: importTorrentFiles(result)
            }
        }
        .presentationDetents([.large])
        .presentationDragIndicator(.visible)
    }

    // MARK: 清单探测（N5 · 解析中）

    private func probeSection(_ probe: ManifestProbe) -> some View {
        let layout = dynamicTypeSize.isAccessibilitySize
            ? AnyLayout(VStackLayout(alignment: .leading, spacing: 12))
            : AnyLayout(HStackLayout(spacing: 12))
        return Section {
            layout {
                HStack(spacing: 12) {
                    ProgressView()
                    VStack(alignment: .leading, spacing: 2) {
                        Text(L("manifestResolvingLabel"))
                        if !probe.host.isEmpty {
                            Text(probe.host)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .lineLimit(1)
                        }
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                Button(L("manifestResolvingCancel")) { form.cancelProbe() }
                    .buttonStyle(.borderless)
            }
            .id(Self.probeId)
        }
    }

    // MARK: 链接

    private func linksSection(_ entries: [UrlEntry]) -> some View {
        let hasText = !form.urlText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        let error: String? =
            if hasText, entries.isEmpty { L("newDownloadNoValidUrl") }
            else if !hasText, form.showEmptyError { L("mobileEnterUrl") }
            else { nil }
        return Section {
            TextEditor(text: $form.urlText)
                .font(.callout.monospaced())
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                .keyboardType(.URL)
                .focused($linksFocused)
                .disabled(form.submitting || form.probe != nil)
                .frame(minHeight: 110)
                .overlay(alignment: .topLeading) {
                    if form.urlText.isEmpty {
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
                .onChange(of: form.urlText) { form.showEmptyError = false }
            if let error {
                Label(error, systemImage: "exclamationmark.circle.fill")
                    .font(.footnote)
                    .foregroundStyle(Color.fdStatusFailedText)
            }
            pasteButton
            torrentButton
            importButton
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
            Text(L("batchDownloadDesc"))
        }
    }

    private var pasteButton: some View {
        PasteButton(payloadType: String.self) { strings in
            let text = strings.joined(separator: "\n")
            Task { @MainActor in
                if text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    FluxHaptic.error.play()
                    container.toasts.show(text: L("mobileClipboardEmpty"), tone: .warning)
                } else {
                    form.appendText(text)
                    form.showEmptyError = false
                    FluxHaptic.success.play()
                    container.toasts.show(text: L("mobilePasted"), tone: .info, systemImage: "list.clipboard")
                }
            }
        }
        .labelStyle(.titleAndIcon)
        .buttonBorderShape(.capsule)
        .disabled(form.submitting)
    }

    private var torrentButton: some View {
        Button {
            importerKind = .torrent
            showImporter = true
        } label: {
            if importingTorrent {
                Label { Text(L("openTorrentFile")) } icon: { ProgressView() }
            } else {
                Label(L("openTorrentFile"), systemImage: "doc.badge.plus")
            }
        }
        .disabled(form.submitting || importingTorrent || form.probe != nil)
        .accessibilityHint(importingTorrent ? L("btProbing") : "")
    }

    private var importButton: some View {
        Button(L("importTxtFile"), systemImage: "doc.text") {
            importerKind = .text
            showImporter = true
        }
        .disabled(form.submitting)
    }

    // MARK: 预览

    private func previewSection(_ entries: [UrlEntry]) -> some View {
        let duplicate = duplicateName(entries)
        return Section {
            ForEach(Array(entries.prefix(3)), id: \.url) { entry in
                PreviewRow(entry: entry)
            }
            if entries.count > 3 {
                Text(L("mobileNewDownloadPreviewMore", ["n": entries.count - 3]))
                    .font(.footnote.monospaced())
                    .foregroundStyle(.secondary)
            }
            if let duplicate {
                Banner(text: L("mobileNewDownloadDuplicateUrl", ["name": duplicate]), tone: .warning, slim: true)
                    .listRowInsets(EdgeInsets())
                    .listRowBackground(Color.clear)
            }
        }
    }

    private func duplicateName(_ entries: [UrlEntry]) -> String? {
        guard let first = entries.first?.url else { return nil }
        return state.tasks.first { $0.url == first || $0.originUrl == first }?.fileName
    }

    // MARK: 目录 / 文件名

    @ViewBuilder
    private func destinationSection(_ entries: [UrlEntry]) -> some View {
        Section {
            if container.isLocalHost {
                LabeledContent(L("saveDir")) {
                    Text(form.saveDir.isEmpty ? LocalPaths.documents.path : form.saveDir)
                        .font(.footnote.monospaced())
                        .lineLimit(2)
                        .truncationMode(.middle)
                        .multilineTextAlignment(.trailing)
                        .textSelection(.enabled)
                }
            } else {
                TextField(L("saveDir"), text: $form.saveDir)
                    .font(.callout.monospaced())
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .disabled(form.submitting)
                if !form.saveDirValid {
                    Label(L("mobileSaveDirInvalid"), systemImage: "exclamationmark.circle.fill")
                        .font(.footnote)
                        .foregroundStyle(Color.fdStatusFailedText)
                }
            }
            if entries.count == 1 {
                TextField(L("renameTask"), text: $form.rename, prompt: Text(L("autoDetectFilename")))
                    .textInputAutocapitalization(.never)
                    .autocorrectionDisabled()
                    .disabled(form.submitting)
            }
        }
    }

    // MARK: 线程 / 队列

    @ViewBuilder
    private func optionsSection(_ entries: [UrlEntry]) -> some View {
        let showThreads = NewDownloadForm.threadsApplicable(entries)
        let queues = state.queues
        if showThreads || !queues.isEmpty {
            Section {
                if showThreads {
                    Picker(L("threads"), selection: threadChoice) {
                        Text(L("auto")).tag(0)
                        ForEach(threadPresets, id: \.self) { n in
                            Text(L("nThreads", ["n": n])).tag(n)
                        }
                        Text(L("customThreads")).tag(-1)
                    }
                    .pickerStyle(.menu)
                    .disabled(form.submitting)
                    if form.threadMode == .custom {
                        Stepper(value: $form.customThreads, in: 1...maxThreads) {
                            Text(L("nThreads", ["n": form.customThreads])).monospacedDigit()
                        }
                        .accessibilityLabel(L("customThreads"))
                    }
                }
                if !queues.isEmpty {
                    Picker(L("taskQueueLabel"), selection: queueChoice(queues)) {
                        ForEach(queues) { q in
                            Text("\(newTaskQueueLabel(q)) · \(q.isRunning ? L("queueRunningBadge") : L("queueStoppedBadge"))")
                                .tag(q.queueId)
                        }
                    }
                    .pickerStyle(.menu)
                    .disabled(form.submitting)
                }
            } footer: {
                if showThreads, form.threadMode == .custom { Text(L("customThreadsHint")) }
            }
        }
    }

    private var threadChoice: Binding<Int> {
        Binding(
            get: {
                switch form.threadMode {
                case .auto: 0
                case .preset: form.presetThreads
                case .custom: -1
                }
            },
            set: { value in
                switch value {
                case 0: form.threadMode = .auto
                case -1: form.threadMode = .custom
                default:
                    form.threadMode = .preset
                    form.presetThreads = value
                }
            }
        )
    }

    /// 表单里的队列不在列表中（列表晚于表单到达 / 队列被删）→ 显示并提交第一个队列。
    private func queueChoice(_ queues: [TaskQueue]) -> Binding<String> {
        Binding(
            get: { currentQueueId(queues) },
            set: { form.queueId = $0 }
        )
    }

    private func currentQueueId(_ queues: [TaskQueue]) -> String {
        queues.contains { $0.queueId == form.queueId } ? form.queueId : (queues.first?.queueId ?? form.queueId)
    }

    // MARK: 高级入口

    private func advancedSection(_ entries: [UrlEntry]) -> some View {
        let single = entries.count == 1
        let singleHttp = single && NewDownloadForm.isHttpLike(entries[0].url)
        let modified = form.advanced.modified(single: single, singleHttp: singleHttp)
        let headerCount = form.advanced.headers.filter { !$0.key.trimmingCharacters(in: .whitespaces).isEmpty }.count
        let summary = modified.isEmpty
            ? L("mobileAdvancedNone")
            : modified.map { advancedLabel($0, headerCount: headerCount) }.joined(separator: " · ")
        return Section {
            NavigationLink(value: NewDownloadRoute.advanced) {
                VStack(alignment: .leading, spacing: 2) {
                    Text(L("taskProxyAdvanced"))
                    Text(summary)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
            }
            .accessibilityElement(children: .combine)
        }
    }

    // MARK: 底部操作栏（系统 `.bottomBar`：只渲染一层 Liquid Glass）

    @ToolbarContentBuilder
    private func bottomBar(_ entries: [UrlEntry]) -> some ToolbarContent {
        let queues = state.queues
        let queueId = currentQueueId(queues)
        let startLabel = entries.count > 1
            ? L("startBatchDownload", ["count": entries.count])
            : L("startDownload")
        let canSubmit = !form.submitting && form.probe == nil && !entries.isEmpty
        ToolbarItem(placement: .bottomBar) {
            Button { submit(startPaused: true, queueId: queueId) } label: {
                if dynamicTypeSize.isAccessibilitySize {
                    Label(L("downloadLater"), systemImage: "clock").labelStyle(.iconOnly)
                } else {
                    Label(L("downloadLater"), systemImage: "clock").labelStyle(.titleAndIcon)
                }
            }
            .disabled(!canSubmit)
        }
        ToolbarSpacer(.flexible, placement: .bottomBar)
        if queues.count > 1 {
            ToolbarItem(placement: .bottomBar) {
                Menu {
                    Section {
                        ForEach(queues) { q in
                            Button(L("mobileStartIntoQueue", ["name": newTaskQueueLabel(q)]), systemImage: "arrow.down") {
                                submit(startPaused: false, queueId: q.queueId)
                            }
                        }
                    }
                    Section {
                        ForEach(queues) { q in
                            Button(L("mobileLaterIntoQueue", ["name": newTaskQueueLabel(q)]), systemImage: "clock") {
                                submit(startPaused: true, queueId: q.queueId)
                            }
                        }
                    }
                } label: {
                    Image(systemName: "ellipsis")
                }
                .disabled(!canSubmit)
                .accessibilityLabel(L("moreActions"))
            }
            ToolbarSpacer(.fixed, placement: .bottomBar)
        }
        ToolbarItem(placement: .bottomBar) {
            Button { submit(startPaused: false, queueId: queueId) } label: {
                if dynamicTypeSize.isAccessibilitySize {
                    Label(startLabel, systemImage: "arrow.down").labelStyle(.iconOnly)
                } else {
                    Label(startLabel, systemImage: "arrow.down").lineLimit(1)
                }
            }
            .buttonStyle(.borderedProminent)
            .disabled(!canSubmit)
            .accessibilityLabel(startLabel)
        }
    }

    // MARK: 动作

    private func requestClose() {
        guard !form.submitting else { return }
        if form.isDirty { showDiscard = true } else { close() }
    }

    private func reject() { FluxHaptic.error.play() }

    private func submit(startPaused: Bool, queueId: String) {
        guard !form.submitting, form.probe == nil else { return }
        let state = container.store.state
        if state.isReadOnly {
            reject()
            container.toasts.show(text: L("localServiceDisconnected"), tone: .error, systemImage: "wifi.slash")
            return
        }
        if form.entries.isEmpty {
            reject()
            form.showEmptyError = true
            return
        }
        if !container.isLocalHost, !form.saveDirValid {
            reject()
            return
        }
        if form.entries.count == 1, !checksumHexValid(form.advanced.checksumHex) {
            reject()
            return
        }
        let manualProxy = manualProxyUrl(state.config)
        let requests = form.buildRequests(startPaused: startPaused, queue: queueId, manualProxy: manualProxy)
        // N5：单条 http(s) 链接先探测插件多文件清单；未命中 / 失败 / 超时都静默回退为普通建任务。
        if requests.count == 1, let request = requests.first, shouldProbeManifest(request.url) {
            probeManifest(request, requests: requests, startPaused: startPaused, queueId: queueId, manualProxy: manualProxy)
            return
        }
        createTasks(requests, startPaused: startPaused)
    }

    private func shouldProbeManifest(_ url: String) -> Bool {
        ManifestSelection.isPreviewable(url) && !TorrentFile.isTorrentFileName(URL(string: url)?.path ?? "")
    }

    /// 行内进度（非阻塞）：探测期间表单保持可见，提交键禁用；关闭 Sheet 取消。
    private func probeManifest(
        _ request: CreateTaskRequest,
        requests: [CreateTaskRequest],
        startPaused: Bool,
        queueId: String,
        manualProxy: String
    ) {
        let url = request.url
        let base = form.manifestBase(queue: queueId, manualProxy: manualProxy)
        let probe = ManifestProbe(
            session: container.session,
            method: HostMethod.daemonGroupResolvePreview,
            params: base.previewRequest(url: url),
            host: URL(string: url)?.host ?? ""
        )
        form.probe = probe
        linksFocused = false
        AccessibilityNotification.Announcement(L("manifestResolvingLabel")).post()
        Task {
            let outcome = await probe.outcome()
            guard form.probe?.id == probe.id else { return }
            form.probe = nil
            if case let .manifest(preview) = outcome {
                FluxHaptic.soft.play()
                path.append(.manifest(.daemon(preview: preview, sourceUrl: url), base))
            } else {
                createTasks(requests, startPaused: startPaused)
            }
        }
    }

    private func createTasks(_ requests: [CreateTaskRequest], startPaused: Bool) {
        form.submitting = true
        let session = container.session
        let toasts = container.toasts
        Task {
            var done = Set<String>()
            var failure: HostError?
            for request in requests {
                do throws(HostError) {
                    _ = try await session.createTask(request)
                    done.insert(request.url)
                } catch {
                    failure = error
                }
            }
            form.submitting = false
            if let failure {
                reject()
                form.removeUrls(done)
                toasts.show(text: ErrorText.describe(failure), tone: .error)
            } else {
                FluxHaptic.success.play()
                let text =
                    if done.count > 1 { L("mobileDownloadStartedN", ["n": done.count]) }
                    else if startPaused { L("taskCreatedToast") }
                    else { L("mobileDownloadStarted") }
                toasts.show(text: text, tone: .success, systemImage: startPaused ? "clock" : "arrow.down")
                close()
            }
        }
    }

    private func importFiles(_ result: Result<[URL], Error>) {
        switch result {
        case let .failure(error):
            container.toasts.show(text: error.localizedDescription, tone: .error)
        case let .success(urls):
            Task { await mergeTextFiles(urls) }
        }
    }

    /// TXT 导入与拖放共用：宽松解析链接，按 URL 去重合并进文本框。
    private func mergeTextFiles(_ urls: [URL]) async {
        var found: [UrlEntry] = []
        var unreadable: [String] = []
        for url in urls {
            let result = await Task.detached { Result { try readTextFile(url) } }.value
            switch result {
            case let .success(text): found += parseEntries(text, loose: true)
            case .failure: unreadable.append(url.lastPathComponent)
            }
        }
        if !unreadable.isEmpty {
            FluxHaptic.error.play()
            container.toasts.show(text: L("importTxtReadFailed", ["name": unreadable.joined(separator: ", ")]), tone: .error)
        }
        found = found.dedupe()
        if found.isEmpty {
            if unreadable.isEmpty {
                FluxHaptic.error.play()
                container.toasts.show(text: L("importTxtNoUrls"), tone: .warning)
            }
        } else {
            let merged = appendEntries(form.urlText, found)
            form.urlText = merged.text
            container.toasts.show(text: L("importTxtFound", ["count": merged.added]), tone: .success, systemImage: "doc.text")
        }
    }

    private func importTorrentFiles(_ result: Result<[URL], Error>) {
        switch result {
        case let .failure(error):
            if (error as? CocoaError)?.code == .userCancelled { return }
            container.toasts.show(text: error.localizedDescription, tone: .error)
        case let .success(urls):
            Task { await importTorrents(urls) }
        }
    }

    /// N3：以表单当前保存目录与所选队列立即提交；表单里仍有链接时 N1 保留，否则关闭。
    private func importTorrents(_ urls: [URL]) async {
        guard !urls.isEmpty, !importingTorrent, !form.submitting else { return }
        if !container.isLocalHost, !form.saveDirValid {
            reject()
            return
        }
        importingTorrent = true
        defer { importingTorrent = false }
        let outcome = await TorrentImport.submit(
            urls,
            container: container,
            saveDir: form.saveDir,
            queueId: currentQueueId(state.queues),
            startPaused: false
        )
        if outcome.created > 0, form.urlText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            close()
        }
    }

    /// iPad 拖放：`.torrent` → 导入；`.txt/.url/.list` → TXT 导入路径；其余 → 不支持提示。
    private func handleDrop(_ providers: [NSItemProvider]) -> Bool {
        guard !providers.isEmpty, !form.submitting else { return false }
        Task {
            let loaded = await DroppedFiles.load(providers)
            defer { DroppedFiles.discard(loaded.files) }
            if loaded.rejected > 0 {
                FluxHaptic.warning.play()
                container.toasts.show(text: L("unsupportedDropHint"), tone: .warning)
            }
            if loaded.failed > 0 {
                FluxHaptic.error.play()
                container.toasts.show(text: L("dropReadFailed", ["count": loaded.failed]), tone: .error)
            }
            let torrents = loaded.files.filter { $0.kind == .torrent }.map(\.url)
            let texts = loaded.files.filter { $0.kind == .text }.map(\.url)
            if !torrents.isEmpty { await importTorrents(torrents) }
            if !texts.isEmpty { await mergeTextFiles(texts) }
        }
        return true
    }

    private func advancedLabel(_ item: AdvancedItem, headerCount: Int) -> String {
        switch item {
        case .auth: L("taskHttpAuth")
        case .proxy: L("taskProxy")
        case .userAgent: L("userAgent")
        case .cookie: L("taskCookie")
        case .referrer: L("mobileReferrer")
        case .checksum: L("taskChecksum")
        case .headers: L("taskHeaders") + " ×\(headerCount)"
        case .tls: L("taskIgnoreTlsErrors")
        }
    }
}

nonisolated enum NewDownloadRoute: Hashable {
    case advanced
    /// 插件清单选择（N5）：来源 + 组级基础选项。
    case manifest(ManifestSource, ManifestBaseOptions)
}

/// 文件选择器的用途：决定可选类型与结果去向。
private nonisolated enum ImporterKind { case text, torrent }

/// 读取文本文件（上限 2 MiB，防止误选大文件）；需要 security-scoped 访问。读取失败向上抛出，由调用方提示。
private nonisolated func readTextFile(_ url: URL) throws -> String {
    let scoped = url.startAccessingSecurityScopedResource()
    defer { if scoped { url.stopAccessingSecurityScopedResource() } }
    let handle = try FileHandle(forReadingFrom: url)
    defer { try? handle.close() }
    let data = try handle.read(upToCount: 2 * 1024 * 1024) ?? Data()
    return String(decoding: data, as: UTF8.self)
}

// MARK: 预览行

private struct PreviewRow: View {
    let entry: UrlEntry
    var body: some View {
        let proto = protocolOf(entry.url)
        let name = inferName(entry)
        let tag = Self.tag(proto)
        let subtitle = hostOrNull(entry.url) ?? tag
        HStack(spacing: 12) {
            KindIcon(kind: proto == .bt ? .torrent : FileKind.from(fileName: name), size: 36)
            VStack(alignment: .leading, spacing: 2) {
                Text(name)
                    .font(.subheadline.monospaced())
                    .lineLimit(1)
                    .truncationMode(.middle)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
            Spacer(minLength: 8)
            ProtocolBadge(text: tag)
        }
        .accessibilityElement(children: .ignore)
        .accessibilityLabel("\(tag), \(name), \(subtitle)")
    }

    static func tag(_ p: TaskProtocol) -> String { TaskDetailFormat.protocolTag(p) }
}
