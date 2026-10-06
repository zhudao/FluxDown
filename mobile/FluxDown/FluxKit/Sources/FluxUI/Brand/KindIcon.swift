import SwiftUI

/// 文件类别（图标 + 类别色，§3.C / §6.2 / §6.4）。
public nonisolated enum FileKind: Sendable, Hashable, CaseIterable {
    case video, audio, document, image, program, archive, ebook, diskImage, application, torrent, other

    /// SF Symbol：行内类别图标（§6.4），磁盘镜像 / 应用 / 种子取 PC `TaskKind` 备用图标（§6.2）。
    public var symbolName: String {
        switch self {
        case .video: "film"
        case .audio: "music.note"
        case .document: "doc.text"
        case .image: "photo"
        case .program: "cpu"
        case .archive: "archivebox"
        case .ebook: "books.vertical"
        case .diskImage: "opticaldisc"
        case .application: "macwindow"
        case .torrent: "link"
        case .other: "doc"
        }
    }

    /// 类别色（§3.C；磁盘镜像随压缩包、应用 / 种子随程序）。
    public var tint: Color {
        switch self {
        case .video: .fdKindVideo
        case .audio: .fdKindAudio
        case .document: .fdKindDocument
        case .image: .fdKindImage
        case .program, .application, .torrent: .fdKindProgram
        case .archive, .diskImage: .fdKindArchive
        case .ebook: .fdKindEbook
        case .other: .fdKindOther
        }
    }

    /// 按文件扩展名推断类别（纯函数，大小写不敏感；无扩展名 / 未知 → `.other`）。
    public nonisolated static func from(fileName: String) -> FileKind {
        guard let dot = fileName.lastIndex(of: "."), dot != fileName.startIndex else { return .other }
        let ext = fileName[fileName.index(after: dot)...].lowercased()
        return extensionTable[ext] ?? .other
    }

    private nonisolated static let extensionTable: [String: FileKind] = {
        let groups: [(FileKind, [String])] = [
            (.video, ["mp4", "mkv", "avi", "mov", "wmv", "flv", "webm", "m4v", "ts", "m3u8", "mpg", "mpeg", "3gp", "rmvb", "mts", "m2ts"]),
            (.audio, ["mp3", "flac", "wav", "aac", "ogg", "m4a", "wma", "opus", "aiff", "ape", "mid", "midi"]),
            (.document, ["pdf", "doc", "docx", "xls", "xlsx", "ppt", "pptx", "txt", "md", "rtf", "csv", "odt", "ods", "odp", "pages", "numbers", "key"]),
            (.image, ["jpg", "jpeg", "png", "gif", "webp", "bmp", "svg", "heic", "heif", "avif", "tif", "tiff", "ico", "raw", "psd"]),
            (.program, ["exe", "msi", "pkg", "deb", "rpm", "apk", "ipa", "appimage", "bat", "sh", "jar"]),
            (.archive, ["zip", "rar", "7z", "tar", "gz", "tgz", "bz2", "xz", "zst", "lz", "z"]),
            (.ebook, ["epub", "mobi", "azw", "azw3", "fb2"]),
            (.diskImage, ["iso", "img", "dmg", "vhd", "vhdx", "vmdk", "bin", "cue"]),
            (.application, ["app"]),
            (.torrent, ["torrent"]),
        ]
        var table: [String: FileKind] = [:]
        for (kind, exts) in groups {
            for ext in exts { table[ext] = kind }
        }
        return table
    }()
}

/// 任务图标右下角的状态角标（§3.C、§11.5：形状 + 对钩 / 三角 / 叹号，不只靠颜色）。
public nonisolated enum KindBadge: Sendable, Hashable, CaseIterable {
    case none, completed, warning, failed

    fileprivate var symbolName: String? {
        switch self {
        case .none: nil
        case .completed: "checkmark.circle.fill"
        case .warning: "exclamationmark.triangle.fill"
        case .failed: "exclamationmark.circle.fill"
        }
    }

    fileprivate var color: Color {
        switch self {
        case .none, .completed: .fdToneSolidSuccess
        case .warning: .fdToneSolidWarning
        case .failed: .fdToneSolidError
        }
    }
}

/// 任务图标方块（§3.C）：连续圆角（边长 × 0.28）squircle + 类别色渐变 + 内高光，白色 SF Symbol。
///
/// - `size` 为 Large 档基准边长（舒适 44 / 紧凑 32 / 详情英雄 72），随 Dynamic Type 缩放，上限 `max(size, 72)`。
/// - `dimmed`：文件已缺失（去饱和 + 降不透明度）。
/// - 纯装饰，对 VoiceOver 隐藏（状态由相邻文字承载）。
public struct KindIcon: View {
    public let kind: FileKind
    public let dimmed: Bool
    public let badge: KindBadge

    private let baseSize: CGFloat
    @ScaledMetric private var scaledSize: CGFloat

    public init(kind: FileKind, size: CGFloat = 44, dimmed: Bool = false, badge: KindBadge = .none) {
        self.kind = kind
        self.dimmed = dimmed
        self.badge = badge
        baseSize = size
        _scaledSize = ScaledMetric(wrappedValue: size, relativeTo: .body)
    }

    private var side: CGFloat { min(scaledSize, max(baseSize, 72)) }

    public var body: some View {
        let shape = RoundedRectangle(cornerRadius: side * 0.28, style: .continuous)
        let tint = kind.tint
        ZStack {
            shape.fill(
                LinearGradient(
                    colors: [tint.mix(with: .white, by: 0.18), tint],
                    startPoint: UnitPoint(x: 0.33, y: 0),
                    endPoint: UnitPoint(x: 0.67, y: 1)
                )
            )
            shape.strokeBorder(
                LinearGradient(colors: [.white.opacity(0.45), .white.opacity(0)], startPoint: .top, endPoint: UnitPoint(x: 0.5, y: 0.45)),
                lineWidth: 0.5
            )
            Image(systemName: kind.symbolName)
                .font(.system(size: side * 0.48, weight: .medium))
                .foregroundStyle(.white)
        }
        .frame(width: side, height: side)
        .shadow(color: .black.opacity(0.12), radius: 1, y: 1)
        .saturation(dimmed ? 0.3 : 1)
        .opacity(dimmed ? 0.7 : 1)
        .overlay(alignment: .bottomTrailing) { badgeView }
        .accessibilityHidden(true)
    }

    @ViewBuilder private var badgeView: some View {
        if let symbol = badge.symbolName {
            let diameter = max(side * 0.4, 14)
            Image(systemName: symbol)
                .symbolRenderingMode(.palette)
                .foregroundStyle(.white, badge.color)
                .font(.system(size: diameter, weight: .bold))
                .background(Color(uiColor: .secondarySystemGroupedBackground), in: .circle)
                .offset(x: diameter * 0.22, y: diameter * 0.22)
        }
    }
}

#Preview("KindIcon · 类别") {
    ScrollView {
        LazyVGrid(columns: [GridItem(.adaptive(minimum: 80))], spacing: 16) {
            ForEach(FileKind.allCases, id: \.self) { kind in
                VStack(spacing: 6) {
                    KindIcon(kind: kind)
                    Text(String(describing: kind)).font(.caption2).foregroundStyle(.secondary)
                }
            }
        }
        .padding()
    }
}

#Preview("KindIcon · 角标 / 缺失 / 深色") {
    HStack(spacing: 20) {
        KindIcon(kind: .video, badge: .completed)
        KindIcon(kind: .archive, badge: .warning)
        KindIcon(kind: .audio, badge: .failed)
        KindIcon(kind: .image, dimmed: true, badge: .warning)
        KindIcon(kind: .document, size: 32)
        KindIcon(kind: .program, size: 72)
    }
    .padding(24)
    .background(Color(uiColor: .secondarySystemGroupedBackground))
    .preferredColorScheme(.dark)
}
