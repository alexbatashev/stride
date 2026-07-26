import Foundation

/// Absolute mount points of the one canonical POSIX namespace clients address by
/// full path. `/home/agent` is a thread's writable workspace; `/home/user` is the
/// user's persistent library. A `nil` thread id routes to the standalone library
/// endpoint (`/api/files`); a present one routes to `/api/threads/{id}/files`.
enum FileMounts {
    static let agentHome = "/home/agent"
    static let userHome = "/home/user"
}

/// Whether an entry is a directory or a regular file. Mirrors the server's
/// `kind` string ("directory" | "file").
enum FileKind: String, Equatable, Decodable, Sendable {
    case directory
    case file
}

/// One row in a directory listing. Mirrors `WorkspaceEntry`/`FileEntry` from the
/// server. The full `path` is used as the stable identity.
struct FileEntry: Identifiable, Equatable, Decodable, Sendable {
    let name: String
    let path: String
    let kind: FileKind
    let size: Int64?
    let updatedAt: Int64
    let mimeType: String?

    var id: String { path }
    var isDirectory: Bool { kind == .directory }

    enum CodingKeys: String, CodingKey {
        case name, path, kind, size
        case updatedAt = "updated_at"
        case mimeType = "mime_type"
    }
}

/// A directory listing response. Mirrors `WorkspaceListResponse`/`FileListResponse`.
struct FileListing: Equatable, Decodable, Sendable {
    let path: String
    let entries: [FileEntry]
}

/// A file the client is about to upload via multipart form data.
struct FileUpload: Equatable, Sendable {
    let name: String
    let mimeType: String?
    let data: Data
}

/// One entry in an upload response. Mirrors the server `UploadedFile`. Workspace
/// uploads return an absolute `/home/agent/...` path the agent can read.
struct UploadedFile: Equatable, Decodable, Sendable {
    let name: String
    let path: String
    let size: Int
}

extension FileEntry {
    var updatedDate: Date {
        Date(timeIntervalSince1970: TimeInterval(updatedAt) / 1000)
    }

    /// Human-readable size, or empty for directories / unknown sizes.
    var sizeLabel: String {
        guard let size, kind == .file else { return "" }
        return ByteCountFormatter.string(fromByteCount: size, countStyle: .file)
    }
}
