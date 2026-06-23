import Foundation

/// One entry in a global-files listing. Mirrors the server's `FileEntry` shape.
struct ProviderFileEntry: Decodable {
    let name: String
    let path: String
    let kind: String
    let size: Int64?
    let updatedAt: Int64
    let mimeType: String?

    var isDirectory: Bool { kind == "directory" }

    enum CodingKeys: String, CodingKey {
        case name, path, kind, size
        case updatedAt = "updated_at"
        case mimeType = "mime_type"
    }
}

private struct ProviderFileListing: Decodable {
    let path: String
    let entries: [ProviderFileEntry]
}

enum FilesServiceError: Error {
    case http(Int)
    case transport
    case decoding
}

/// Minimal REST client for the user's global files (`/api/files`). Decoupled
/// from the host app's client so the extension target stays self-contained.
struct StrideFilesService {
    let credentials: StrideCredentials
    private let session = URLSession(configuration: .default)

    init?(credentials: StrideCredentials? = StrideCredentials.current()) {
        guard let credentials else { return nil }
        self.credentials = credentials
    }

    func list(path: String) async throws -> [ProviderFileEntry] {
        let data = try await send(get(endpoint("/api/files", query: path)))
        do {
            return try JSONDecoder().decode(ProviderFileListing.self, from: data).entries
        } catch {
            throw FilesServiceError.decoding
        }
    }

    func read(path: String) async throws -> Data {
        try await send(get(fileEndpoint("/api/files", path: path)))
    }

    func delete(path: String) async throws {
        var request = get(fileEndpoint("/api/files", path: path))
        request.httpMethod = "DELETE"
        _ = try await send(request)
    }

    func createDirectory(path: String) async throws {
        var request = get(url("/api/files/directories"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(["path": path])
        _ = try await send(request)
    }

    func rename(path: String, to name: String) async throws {
        var request = get(url("/api/files/rename"))
        request.httpMethod = "PATCH"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(["path": path, "name": name])
        _ = try await send(request)
    }

    func upload(directory: String, name: String, data: Data, mime: String?) async throws {
        let boundary = "stride-\(UUID().uuidString)"
        var request = get(endpoint("/api/files", query: directory))
        request.httpMethod = "POST"
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        request.httpBody = Self.multipart(name: name, data: data, mime: mime, boundary: boundary)
        _ = try await send(request)
    }

    // MARK: Request plumbing

    private func get(_ url: URL) -> URLRequest {
        var request = URLRequest(url: url)
        request.setValue("Bearer \(credentials.token)", forHTTPHeaderField: "Authorization")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        return request
    }

    @discardableResult
    private func send(_ request: URLRequest) async throws -> Data {
        do {
            let (data, response) = try await session.data(for: request)
            guard let http = response as? HTTPURLResponse else { throw FilesServiceError.transport }
            guard (200..<300).contains(http.statusCode) else { throw FilesServiceError.http(http.statusCode) }
            return data
        } catch let error as FilesServiceError {
            throw error
        } catch {
            throw FilesServiceError.transport
        }
    }

    private func url(_ path: String) -> URL {
        var base = credentials.baseURL.absoluteString
        if base.hasSuffix("/") { base.removeLast() }
        return URL(string: base + path) ?? credentials.baseURL
    }

    private func endpoint(_ prefix: String, query: String) -> URL {
        url("\(prefix)?path=\(Self.encodeQuery(query))")
    }

    private func fileEndpoint(_ prefix: String, path: String) -> URL {
        url("\(prefix)/\(Self.encodePath(path))")
    }

    private static func encodePath(_ path: String) -> String {
        let trimmed = path.hasPrefix("/") ? String(path.dropFirst()) : path
        return trimmed.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? trimmed
    }

    private static func encodeQuery(_ value: String) -> String {
        var allowed = CharacterSet.urlQueryAllowed
        allowed.remove(charactersIn: "+&=?/")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }

    private static func multipart(name: String, data: Data, mime: String?, boundary: String) -> Data {
        var body = Data()
        let newline = "\r\n"
        body.appendString("--\(boundary)\(newline)")
        body.appendString("Content-Disposition: form-data; name=\"file\"; filename=\"\(name)\"\(newline)")
        body.appendString("Content-Type: \(mime ?? "application/octet-stream")\(newline)\(newline)")
        body.append(data)
        body.appendString(newline)
        body.appendString("--\(boundary)--\(newline)")
        return body
    }
}

/// Writes downloaded bytes to a unique temp file so the File Provider can hand a
/// URL back to the system.
func providerTemporaryFile(named name: String, data: Data) throws -> URL {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("stride-fp", isDirectory: true)
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let url = directory.appendingPathComponent(name.isEmpty ? "file" : name)
    try data.write(to: url, options: .atomic)
    return url
}

private extension Data {
    mutating func appendString(_ string: String) {
        if let data = string.data(using: .utf8) { append(data) }
    }
}
