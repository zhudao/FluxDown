import FluxDomain
import QuickLook
import SwiftUI

/// 任务上下文菜单 / 详情「更多」菜单的内容，顺序沿用 PC `MenuEntry`（同 Android `TaskActions.menuItems`）：
/// 打开 / 重新下载 / 继续·暂停 / 优先 ─ 复制链接 / 分享 / 重命名 / 更换下载源 / 移动到队列 / 选择 ─ 删除。
struct TaskMenuItems: View {
    let task: DownloadTask
    /// 当前是否为优先任务（`HostState.priorityTaskId`）。
    let boosted: Bool
    /// 列表多选入口（详情页不传）。
    var onSelect: (() -> Void)?

    @Environment(TaskActions.self) private var actions

    var body: some View {
        let status = task.status
        Section {
            if actions.hasLocalFile(task) {
                Button(L("openFile"), systemImage: "arrow.up.right.square") { actions.open(task) }
                Button(L("mobileShowInFiles"), systemImage: "folder") { actions.showInFiles(task) }
            }
            if actions.canRedownload(task) {
                Button(L("redownloadTask"), systemImage: "arrow.clockwise") { actions.confirmRedownload(task) }
            }
            if status == .paused || status == .failed {
                Button(L("resume"), systemImage: "play.fill") { actions.resume([task.taskId]) }
            }
            if status.isActive || status == .pending {
                Button(L("pause"), systemImage: "pause.fill") { actions.pause([task.taskId]) }
            }
            if status != .completed {
                Toggle(isOn: Binding(get: { boosted }, set: { _ in actions.boost(task, boosted: boosted) })) {
                    Label(L(boosted ? "cancelBoost" : "boostDownload"), systemImage: "bolt.fill")
                }
            }
        }
        Section {
            Button(L("copyUrl"), systemImage: "doc.on.doc") { actions.copyLink(task) }
            ShareLink(item: task.shareUrl) { Label(L("mobileShareLink"), systemImage: "square.and.arrow.up") }
            if !status.isActive, task.protocol != .bt {
                Button(L("renameTask"), systemImage: "pencil") { actions.rename(task) }
            }
            if status == .failed || status == .paused {
                Button(L("mobileChangeUrl"), systemImage: "link") { actions.changeUrl(task) }
            }
            if status != .completed {
                Button(L("moveToQueueAction"), systemImage: "list.number") { actions.moveToQueue([task.taskId]) }
            }
            if let onSelect {
                Button(L("mobileMenuSelect"), systemImage: "checkmark.circle") { onSelect() }
            }
        }
        Section {
            Button(L("delete"), systemImage: "trash", role: .destructive) { actions.confirmDelete([task]) }
        }
    }
}

extension View {
    /// 根视图挂载：呈现 `TaskActions.dialog`（删除 / 重新下载确认、重命名 / 更换下载源输入）与快速查看。
    func taskActionDialogs(_ actions: TaskActions) -> some View {
        modifier(TaskActionDialogs(actions: actions))
    }
}

private struct TaskActionDialogs: ViewModifier {
    @Bindable var actions: TaskActions
    @State private var text = ""

    private var deleting: [DownloadTask] {
        if case let .delete(tasks, _)? = actions.dialog { return tasks }
        return []
    }

    private func binding(_ match: @escaping (TaskDialog) -> Bool) -> Binding<Bool> {
        Binding(
            get: { actions.dialog.map(match) ?? false },
            set: { if !$0 { actions.dialog = nil } }
        )
    }

    func body(content: Content) -> some View {
        content
            .confirmationDialog(
                deleting.count == 1 ? L("deleteTask") : L("mobileDeleteNTitle", ["n": deleting.count]),
                isPresented: binding { if case .delete = $0 { return true } else { return false } },
                titleVisibility: .visible,
                presenting: actions.dialog
            ) { dialog in
                if case let .delete(tasks, onDone) = dialog {
                    let ids = tasks.map(\.taskId)
                    Button(L("deleteTaskAndFile"), role: .destructive) { actions.delete(ids, withFiles: true, onDone: onDone) }
                    Button(L("deleteTask"), role: .destructive) { actions.delete(ids, withFiles: false, onDone: onDone) }
                    Button(L("cancel"), role: .cancel) {}
                }
            } message: { dialog in
                if case let .delete(tasks, _) = dialog {
                    Text(tasks.count == 1 ? L("deleteConfirmDescKeepFile", ["fileName": tasks[0].fileName]) : L("mobileDeleteNMessage"))
                }
            }
            .alert(
                L("redownloadTask"),
                isPresented: binding { if case .redownload = $0 { return true } else { return false } },
                presenting: actions.dialog
            ) { dialog in
                if case let .redownload(task) = dialog {
                    Button(L("cancel"), role: .cancel) {}
                    Button(L("redownloadTask")) { actions.redownload(task) }
                }
            } message: { dialog in
                if case let .redownload(task) = dialog {
                    Text(L("mobileRedownloadMessage", ["fileName": task.fileName]))
                }
            }
            .alert(
                L("renameTaskTitle"),
                isPresented: binding { if case .rename = $0 { return true } else { return false } },
                presenting: actions.dialog
            ) { dialog in
                if case let .rename(task) = dialog {
                    TextField(L("renameTaskPlaceholder"), text: $text)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button(L("cancel"), role: .cancel) {}
                    Button(L("confirm")) { actions.submitRename(task, to: text) }
                }
            }
            .alert(
                L("mobileChangeUrl"),
                isPresented: binding { if case .changeUrl = $0 { return true } else { return false } },
                presenting: actions.dialog
            ) { dialog in
                if case let .changeUrl(task) = dialog {
                    TextField(L("urlPlaceholder"), text: $text)
                        .keyboardType(.URL)
                        .textInputAutocapitalization(.never)
                        .autocorrectionDisabled()
                    Button(L("cancel"), role: .cancel) {}
                    Button(L("confirm")) { actions.submitChangeUrl(task, to: text) }
                }
            }
            .onChange(of: actions.dialog?.id) {
                switch actions.dialog {
                case let .rename(task)?: text = task.fileName
                case let .changeUrl(task)?: text = task.shareUrl
                default: break
                }
            }
            .quickLookPreview($actions.previewURL)
    }
}
