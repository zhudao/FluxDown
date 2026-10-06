import FluxDomain
import FluxUI
import SwiftUI

/// 工具栏「视图」菜单（02-downloads §3.3，系统 `Menu`）：分组 · 排序（+ 方向）· 密度 · 卡片字段 · 任务动作。
/// 偏好是设备本地的（`ViewPrefsStore`），不随主机切换。
struct DownloadsViewMenu: View {
    @Bindable var prefs: ViewPrefsStore
    let model: DownloadsModel
    @Environment(AppContainer.self) private var container
    @Environment(TaskActions.self) private var actions
    @Environment(ToastCenter.self) private var toasts
    let onClearFinished: () -> Void

    var body: some View {
        Menu {
            groupMenu
            sortMenu
            densityMenu
            fieldsMenu
            Section(L("mobileViewTasks")) {
                Button(L("mobileViewSelectTasks"), systemImage: "checklist") { model.beginSelecting() }
                Button(L("pauseAll"), systemImage: "pause.fill") { actions.pauseAll() }
                Button(L("resumeAll"), systemImage: "play.fill") { actions.resumeAll() }
                Button(L("mobileViewClearFinished"), systemImage: "checkmark.circle", action: onClearFinished)
            }
            Section {
                Button(L("manageQueueAction"), systemImage: "list.number") { container.router.sheet = .queues }
            }
            Section {
                Button(L("viewResetDefault"), systemImage: "arrow.counterclockwise", action: reset)
            }
        } label: {
            Label(L("viewMenuLabel"), systemImage: "slider.horizontal.3")
        }
        .accessibilityValue(isCustomized ? L("mobileViewCustomized") : "")
    }

    /// 与默认不同 → 视图按钮显示 on 态（同 Android：含队列范围）。
    var isCustomized: Bool { prefs.isCustomized || model.filter.queueId != nil }

    private var groupMenu: some View {
        Menu(L("viewSectionGroupBy"), systemImage: "rectangle.3.group") {
            Picker(L("viewSectionGroupBy"), selection: $prefs.groupBy) {
                ForEach(GroupBy.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.inline)
        }
    }

    private var sortMenu: some View {
        Menu(L("viewSectionSort"), systemImage: "arrow.up.arrow.down") {
            Picker(L("viewSectionSort"), selection: sortKeyBinding) {
                ForEach(SortKey.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.inline)
            // 智能排序忽略方向。
            Picker(L("viewSectionSort"), selection: $prefs.ascending) {
                Label(L("viewSortAscending"), systemImage: "arrow.up").tag(true)
                Label(L("viewSortDescending"), systemImage: "arrow.down").tag(false)
            }
            .pickerStyle(.inline)
            .disabled(prefs.sortKey == .smart)
        }
    }

    /// 切换排序键：方向重置为该键的默认方向（名称升序，其余降序）。
    private var sortKeyBinding: Binding<SortKey> {
        Binding(
            get: { prefs.sortKey },
            set: { key in
                guard key != prefs.sortKey else { return }
                prefs.sortKey = key
                prefs.ascending = key == .name
            }
        )
    }

    private var densityMenu: some View {
        Menu(L("viewSectionDensity"), systemImage: "list.bullet") {
            Picker(L("viewSectionDensity"), selection: $prefs.density) {
                ForEach(Density.allCases) { Text($0.title).tag($0) }
            }
            .pickerStyle(.inline)
        }
    }

    /// 卡片字段（多选，至少保留 1 项，菜单保持打开）。
    private var fieldsMenu: some View {
        Menu(L("mobileViewFields"), systemImage: "square.text.square") {
            ForEach(CardField.allCases) { field in
                Toggle(field.title, isOn: Binding(
                    get: { prefs.fields.contains(field) },
                    set: { on in
                        if !on, prefs.fields.count == 1 { return }
                        if on != prefs.fields.contains(field) { prefs.toggle(field) }
                    }
                ))
                .menuActionDismissBehavior(.disabled)
            }
        }
    }

    private func reset() {
        prefs.groupBy = .none
        prefs.sortKey = .smart
        prefs.ascending = false
        prefs.density = .comfortable
        prefs.fields = ViewPrefsStore.defaultFields
        model.setQueue(nil)
        toasts.show(text: L("viewResetToast"), tone: .success, systemImage: "arrow.counterclockwise")
    }
}
