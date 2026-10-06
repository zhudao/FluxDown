import FluxDomain
import FluxUI
import SwiftUI

/// 「订阅」标签根页。
/// - compact：`NavigationStack`，订阅列表（R1）→ 推入条目流（R2）；
/// - regular（iPad / 宽窗口）：`NavigationSplitView` 订阅 | 条目 两栏。
/// 订阅属于主机（本机或 `--server`），动作经 `daemon.rss.*`；`daemon.*` 能力恒可达，不做门控。内容层全部是系统 `List`，不上玻璃。
struct RssScreen: View {
    @Environment(AppContainer.self) private var container
    @Environment(TaskActions.self) private var actions

    var body: some View {
        RssContent(container: container, actions: actions)
    }
}

private struct RssContent: View {
    @Environment(AppContainer.self) private var container
    @Environment(TaskActions.self) private var actions
    @Environment(\.horizontalSizeClass) private var sizeClass
    @State private var rss: RssModel
    /// compact 推入的订阅 id 栈。
    @State private var path: [String] = []

    init(container: AppContainer, actions: TaskActions) {
        _rss = State(initialValue: RssModel(container: container, actions: actions))
    }

    var body: some View {
        @Bindable var rss = rss
        let sources = container.store.state.rssSources
        Group {
            if sizeClass == .regular {
                splitLayout(sources: sources)
            } else {
                stackLayout
            }
        }
        .sheet(item: $rss.editor) { target in
            RssEditorSheet(target: target, container: container, rss: rss)
        }
        // 全局搜索「新建订阅」：标签被选中且搜索 Sheet 收起后打开创建编辑器。
        .task(id: container.router.pendingIntent) {
            guard container.router.pendingIntent == .newRssSource, await container.router.claim(.newRssSource) else { return }
            rss.openEditor(.create)
        }
        .confirmationDialog(
            L("rssDeleteSource"),
            isPresented: Binding(
                get: { rss.pendingDelete != nil },
                set: { if !$0 { rss.pendingDelete = nil } }
            ),
            titleVisibility: .visible,
            presenting: rss.pendingDelete
        ) { source in
            Button(L("rssDeleteSource"), role: .destructive) { rss.confirmDelete(source) }
        } message: { source in
            Text(L("rssDeleteConfirmDesc", ["name": RssFormat.title(of: source)]))
        }
        .onChange(of: sources.map { "\($0.sourceId):\($0.unreadCount)" }, initial: true) {
            rss.trackUnread(sources)
        }
        .onChange(of: sources.map(\.sourceId)) { _, ids in
            path.removeAll { !ids.contains($0) }
            if let selected = rss.selectedId, !ids.contains(selected) { rss.selectedId = nil }
        }
    }

    private var stackLayout: some View {
        NavigationStack(path: $path) {
            RssFeedsList(rss: rss, mode: .stack)
                .navigationDestination(for: String.self) { sourceId in
                    RssItemsScreen(sourceId: sourceId, container: container, actions: actions, rss: rss)
                        .id(sourceId)
                }
        }
    }

    private func splitLayout(sources: [RssSource]) -> some View {
        NavigationSplitView {
            RssFeedsList(rss: rss, mode: .split)
                .navigationSplitViewColumnWidth(min: 320, ideal: 380, max: 480)
        } detail: {
            NavigationStack {
                if let id = rss.selectedId, sources.contains(where: { $0.sourceId == id }) {
                    RssItemsScreen(sourceId: id, container: container, actions: actions, rss: rss)
                        .id(id)
                } else {
                    ContentUnavailableView {
                        Label(L("rssSubscriptions"), systemImage: "dot.radiowaves.up.forward")
                    } description: {
                        Text(L("mobileRssSelectFeedHint"))
                    }
                }
            }
        }
    }
}

// MARK: - R1 订阅列表

private struct RssFeedsList: View {
    nonisolated enum Mode { case stack, split }

    @Environment(AppContainer.self) private var container
    let rss: RssModel
    let mode: Mode

    var body: some View {
        @Bindable var rss = rss
        let state = container.store.state
        let sources = state.rssSources
        let visible = rss.visibleSources(sources)
        let failing = sources.filter { $0.failCount > 0 }.count
        let unread = sources.reduce(0) { $0 + Int($1.unreadCount) }
        let readOnly = state.isReadOnly
        Group {
            if mode == .split {
                List(selection: $rss.selectedId) { listContent(sources: sources, visible: visible, failing: failing, unread: unread, readOnly: readOnly, state: state) }
            } else {
                List { listContent(sources: sources, visible: visible, failing: failing, unread: unread, readOnly: readOnly, state: state) }
            }
        }
        .listStyle(.insetGrouped)
        .overlay { overlayState(sources: sources, visible: visible, connection: state.connection) }
        .refreshable { await rss.refreshAll(sources) }
        .navigationTitle(L("rssSubscriptions"))
        .modifier(OptionalSearch(enabled: sources.count >= RssModel.searchThreshold, text: $rss.query))
        .toolbar { toolbarContent(sources: sources, readOnly: readOnly, connection: state.connection) }
        .fluxAnimation(.smooth, value: visible.map(\.sourceId))
    }

    // MARK: 内容

    @ViewBuilder
    private func listContent(sources: [RssSource], visible: [RssSource], failing: Int, unread: Int, readOnly: Bool, state: HostState) -> some View {
        if showsOfflineBanner(state.connection) {
            Section {
                Banner(text: L("localServiceDisconnected"), tone: .warning, systemImage: "wifi.slash")
                    .listRowBackground(Color.clear)
                    .listRowInsets(EdgeInsets())
            }
        }
        if let feedback = rss.feedback {
            Section {
                Banner(
                    text: feedback,
                    tone: .info,
                    systemImage: "tray.and.arrow.down",
                    slim: true,
                    action: BannerAction(title: L("close")) { rss.feedback = nil }
                )
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
        }
        if failing > 0 {
            Section {
                Banner(
                    text: L("mobileRssFailedBanner", ["n": failing]),
                    tone: .warning,
                    systemImage: "exclamationmark.triangle.fill",
                    action: BannerAction(title: L(rss.failingOnly ? "mobileRssShowAll" : "mobileRssShowFailing")) {
                        rss.failingOnly.toggle()
                    }
                )
                .listRowBackground(Color.clear)
                .listRowInsets(EdgeInsets())
            }
        }
        if !sources.isEmpty {
            Section {
                StatRow(cells: [
                    StatCell(value: String(unread), unit: nil, label: L("mobileRssStatUnread"), emphasis: unread > 0 ? .accent : .none),
                    StatCell(value: String(sources.count), unit: nil, label: L("mobileRssStatFeeds")),
                    StatCell(value: String(failing), unit: nil, label: L("mobileRssStatFailing"), emphasis: failing > 0 ? .failure : .none),
                ])
                .padding(.vertical, 4)
            }
            Section {
                ForEach(visible) { source in
                    row(source, readOnly: readOnly)
                }
            } header: {
                Text("\(L("rssSubscriptions")) · \(visible.count)")
            }
        }
    }

    @ViewBuilder
    private func row(_ source: RssSource, readOnly: Bool) -> some View {
        let refreshing = rss.busy.contains(source.sourceId)
        Group {
            if mode == .stack {
                NavigationLink(value: source.sourceId) {
                    RssFeedRow(source: source, refreshing: refreshing)
                }
            } else {
                RssFeedRow(source: source, refreshing: refreshing)
                    .tag(source.sourceId)
            }
        }
        .swipeActions(edge: .leading, allowsFullSwipe: true) {
            if !readOnly {
                Button(L("rssRefreshNow"), systemImage: "arrow.clockwise") { rss.refresh(source) }
                    .tint(.accentColor)
                    .disabled(refreshing)
                if source.unreadCount > 0 {
                    Button(L("rssMarkAllRead"), systemImage: "checkmark.circle") { rss.markAllRead(source) }
                        .tint(Color.fdStatusPaused)
                }
            }
        }
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            if !readOnly {
                Button(L("rssDeleteSource"), systemImage: "trash", role: .destructive) { rss.requestDelete(source) }
                Button(L("rssManageTitle"), systemImage: "slider.horizontal.3") {
                    rss.openEditor(.edit(sourceId: source.sourceId))
                }
                .tint(Color.fdBoost)
            }
        }
        .contextMenu {
            Button(L("rssManageTitle"), systemImage: "slider.horizontal.3") {
                rss.openEditor(.edit(sourceId: source.sourceId))
            }
            .disabled(readOnly)
            Button(L("rssRefreshNow"), systemImage: "arrow.clockwise") { rss.refresh(source) }
                .disabled(refreshing || readOnly)
            Button(L("rssMarkAllRead"), systemImage: "checkmark.circle") { rss.markAllRead(source) }
                .disabled(source.unreadCount == 0 || readOnly)
            Button(L("copyUrl"), systemImage: "doc.on.doc") { rss.copyLink(source) }
            Button(L(source.enabled ? "mobileRssDisable" : "mobileRssEnable"), systemImage: source.enabled ? "pause.circle" : "play.circle") {
                rss.toggle(source)
            }
            .disabled(readOnly)
            Divider()
            Button(L("rssDeleteSource"), systemImage: "trash", role: .destructive) { rss.requestDelete(source) }
                .disabled(readOnly)
        }
    }

    // MARK: 空 / 加载

    @ViewBuilder
    private func overlayState(sources: [RssSource], visible: [RssSource], connection: Connection) -> some View {
        if sources.isEmpty {
            if connection == .connecting {
                ProgressView().controlSize(.large)
            } else {
                ContentUnavailableView {
                    Label(L("mobileRssEmptyTitle"), systemImage: "dot.radiowaves.up.forward")
                } description: {
                    Text(L("rssSidebarEmptyHint"))
                } actions: {
                    Button(L("rssAddSource")) { rss.openEditor(.create) }
                        .buttonStyle(.borderedProminent)
                        .disabled(container.store.state.isReadOnly)
                }
            }
        } else if visible.isEmpty {
            if !rss.query.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                ContentUnavailableView.search(text: rss.query)
            } else {
                ContentUnavailableView {
                    Label(L("mobileRssFilterEmpty"), systemImage: "line.3.horizontal.decrease.circle")
                } actions: {
                    Button(L("mobileRssClearFilter")) {
                        rss.failingOnly = false
                        rss.unreadOnly = false
                    }
                }
            }
        }
    }

    // MARK: 工具栏

    @ToolbarContentBuilder
    private func toolbarContent(sources: [RssSource], readOnly: Bool, connection: Connection) -> some ToolbarContent {
        ToolbarItem(placement: .topBarLeading) { hostPill(connection: connection) }
        ToolbarItem(placement: .primaryAction) { GlobalSearchButton() }
        ToolbarSpacer(.fixed, placement: .primaryAction)
        ToolbarItem(placement: .primaryAction) {
            Button(L("rssAddSource"), systemImage: "plus") { rss.openEditor(.create) }
                .disabled(readOnly)
        }
        if !sources.isEmpty {
            ToolbarItem(placement: .primaryAction) {
                moreMenu(sources: sources, readOnly: readOnly)
            }
        }
    }

    private func moreMenu(sources: [RssSource], readOnly: Bool) -> some View {
        @Bindable var rss = rss
        return Menu {
            Button(L("mobileRssRefreshAll"), systemImage: "arrow.clockwise") {
                Task { await rss.refreshAll(sources) }
            }
            .disabled(readOnly || !rss.busy.isEmpty)
            Button(L("rssMarkAllRead"), systemImage: "checkmark.circle") {
                rss.markAllRead(sources.filter { $0.unreadCount > 0 })
            }
            .disabled(readOnly || !sources.contains { $0.unreadCount > 0 })
            Toggle(L("mobileRssUnreadOnly"), systemImage: "circle.fill", isOn: $rss.unreadOnly)
            Picker(L("mobileRssSort"), selection: $rss.sort) {
                ForEach(RssFeedSort.allCases) { sort in
                    Text(L(sort.titleKey)).tag(sort)
                }
            }
            .pickerStyle(.menu)
        } label: {
            Label(L("moreActions"), systemImage: "ellipsis")
        }
    }

    /// 断连宽限后（stale / failed）才提示；冷启动的 `connecting` 不闪横幅。
    private func showsOfflineBanner(_ connection: Connection) -> Bool {
        switch connection {
        case .stale, .failed: true
        case .live, .connecting: false
        }
    }

    /// 当前主机（订阅属于主机，不是本机偏好）；点按去「设备」页切换主机。
    private func hostPill(connection: Connection) -> some View {
        let name = container.host.localizedName
        return Button {
            container.router.tab = .devices
        } label: {
            HStack(spacing: 6) {
                Circle()
                    .fill(connection == .live ? Color.fdStatusSeeding : Color.fdStatusWarning)
                    .frame(width: 8, height: 8)
                    .accessibilityHidden(true)
                Text(name).lineLimit(1).truncationMode(.middle)
            }
            .frame(maxWidth: 180)
        }
        .accessibilityLabel(L("mobileRssSwitchHostDesc", ["name": name]))
    }
}

/// 订阅数达到阈值才出现搜索栏（R1）。
private struct OptionalSearch: ViewModifier {
    let enabled: Bool
    @Binding var text: String

    @ViewBuilder
    func body(content: Content) -> some View {
        if enabled {
            content.searchable(
                text: $text, placement: .navigationBarDrawer(displayMode: .always), prompt: L("mobileRssSearchFeeds")
            )
        } else {
            content
        }
    }
}
