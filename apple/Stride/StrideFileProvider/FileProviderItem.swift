import FileProvider
import UniformTypeIdentifiers

/// Maps between VFS path strings and `NSFileProviderItemIdentifier`s. The root
/// of the global library is `.rootContainer`; every other item's identifier is
/// just its VFS path (e.g. `docs/report.pdf`).
enum ItemID {
    static func path(for identifier: NSFileProviderItemIdentifier) -> String {
        if identifier == .rootContainer { return "" }
        return identifier.rawValue
    }

    static func identifier(forPath path: String) -> NSFileProviderItemIdentifier {
        path.isEmpty ? .rootContainer : NSFileProviderItemIdentifier(path)
    }

    static func parentPath(of path: String) -> String {
        var segments = path.split(separator: "/")
        guard !segments.isEmpty else { return "" }
        segments.removeLast()
        return segments.joined(separator: "/")
    }
}

/// A single file or folder presented to the system File Provider.
final class FileProviderItem: NSObject, NSFileProviderItem {
    private let identifier: NSFileProviderItemIdentifier
    private let parent: NSFileProviderItemIdentifier
    private let entryName: String
    private let isDirectory: Bool
    private let size: Int64?
    private let updatedAt: Int64
    private let mime: String?

    init(
        identifier: NSFileProviderItemIdentifier,
        parent: NSFileProviderItemIdentifier,
        name: String,
        isDirectory: Bool,
        size: Int64?,
        updatedAt: Int64,
        mime: String?
    ) {
        self.identifier = identifier
        self.parent = parent
        self.entryName = name
        self.isDirectory = isDirectory
        self.size = size
        self.updatedAt = updatedAt
        self.mime = mime
    }

    /// The container for the user's global library.
    static func root() -> FileProviderItem {
        FileProviderItem(
            identifier: .rootContainer,
            parent: .rootContainer,
            name: "S.T.R.I.D.E.",
            isDirectory: true,
            size: nil,
            updatedAt: 0,
            mime: nil
        )
    }

    static func from(_ entry: ProviderFileEntry, parentPath: String) -> FileProviderItem {
        FileProviderItem(
            identifier: ItemID.identifier(forPath: entry.path),
            parent: ItemID.identifier(forPath: parentPath),
            name: entry.name,
            isDirectory: entry.isDirectory,
            size: entry.size,
            updatedAt: entry.updatedAt,
            mime: entry.mimeType
        )
    }

    var itemIdentifier: NSFileProviderItemIdentifier { identifier }
    var parentItemIdentifier: NSFileProviderItemIdentifier { parent }
    var filename: String { entryName }

    var contentType: UTType {
        if isDirectory { return .folder }
        if let mime, let type = UTType(mimeType: mime) { return type }
        let ext = (entryName as NSString).pathExtension
        return UTType(filenameExtension: ext) ?? .data
    }

    var capabilities: NSFileProviderItemCapabilities {
        if isDirectory {
            return [.allowsReading, .allowsContentEnumerating, .allowsAddingSubItems,
                    .allowsDeleting, .allowsRenaming]
        }
        return [.allowsReading, .allowsWriting, .allowsDeleting, .allowsRenaming]
    }

    var documentSize: NSNumber? { size.map { NSNumber(value: $0) } }

    var contentModificationDate: Date? {
        updatedAt > 0 ? Date(timeIntervalSince1970: TimeInterval(updatedAt) / 1000) : nil
    }

    /// Versions the system uses to detect changes. Both derive from the server's
    /// update timestamp, so a remote edit invalidates the cached copy.
    var itemVersion: NSFileProviderItemVersion {
        let token = Data("\(updatedAt)".utf8)
        return NSFileProviderItemVersion(contentVersion: token, metadataVersion: token)
    }
}
