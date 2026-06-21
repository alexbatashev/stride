import Foundation

extension FridayClient {
    /// Builds a client backed by `URLSession`, reading the active server URL and
    /// bearer token from `session` for every call.
    static func live(session: Session) -> FridayClient {
        let api = HTTPAPI(session: session)

        return FridayClient(
            login: { baseURL, username, password in
                try await api.authenticate(path: "/api/login", baseURL: baseURL, username: username, password: password)
            },
            register: { baseURL, username, password in
                try await api.authenticate(path: "/api/register", baseURL: baseURL, username: username, password: password)
            },
            signOut: {
                _ = try? await api.post("/api/logout")
                session.signOut()
            },
            listProjects: {
                try await api.get("/api/projects", as: [Project].self)
            },
            listThreads: {
                try await api.get("/api/threads", as: [ThreadSummary].self)
            },
            listMessages: { threadID in
                try await api.get("/api/threads/\(threadID)/messages", as: [Message].self)
            },
            createThread: { content, projectID, filePaths in
                try await api.post(
                    "/api/threads",
                    body: CreateThreadBody(content: content, projectID: projectID, filePaths: filePaths),
                    as: SendResult.self
                )
            },
            sendMessage: { threadID, content, filePaths in
                try await api.post(
                    "/api/threads/\(threadID)/messages",
                    body: SendMessageBody(content: content, filePaths: filePaths),
                    as: SendResult.self
                )
            },
            cancelRun: { threadID in
                try await api.post("/api/threads/\(threadID)/cancel")
            },
            resolveApproval: { threadID, approvalID, approved in
                try await api.post(
                    "/api/threads/\(threadID)/approvals/\(approvalID)",
                    body: ApprovalBody(approved: approved)
                )
            },
            answerQuiz: { threadID, quizID, answers in
                try await api.post(
                    "/api/threads/\(threadID)/quizzes/\(quizID)",
                    body: QuizAnswerBody(answers: answers)
                )
            },
            events: { threadID in
                api.eventStream(threadID: threadID)
            },
            listFiles: { scope, path in
                try await api.get(api.filesPath(scope, query: path), as: FileListing.self)
            },
            createDirectory: { scope, path in
                try await api.post(api.directoriesPath(scope), body: PathBody(path: path))
            },
            renameFile: { path, newName in
                try await api.patch("/api/files/rename", body: RenameBody(path: path, name: newName))
            },
            deleteFile: { scope, path in
                try await api.delete(api.filePath(scope, path: path))
            },
            uploadFiles: { scope, directory, files in
                try await api.upload(api.filesPath(scope, query: directory), files: files)
            },
            downloadFile: { scope, path in
                try await api.getData(api.filePath(scope, path: path))
            },
            listAutomations: {
                try await api.get("/api/automations", as: [Automation].self)
            },
            createAutomation: { automation in
                try await api.post(
                    "/api/automations",
                    body: CreateAutomationBody(automation),
                    as: Automation.self
                )
            },
            runAutomation: { id in
                try await api.post("/api/automations/\(id)/run")
            },
            setAutomationEnabled: { id, enabled in
                try await api.patch("/api/automations/\(id)", body: EnabledBody(enabled: enabled))
            },
            deleteAutomation: { id in
                try await api.delete("/api/automations/\(id)")
            },
            listAutomationRuns: { id in
                try await api.get("/api/automations/\(id)/runs", as: [AutomationRun].self)
            },
            listEmailAccounts: {
                try await api.get("/api/settings/email", as: [EmailAccount].self)
            },
            serverBaseURL: {
                session.baseURL
            }
        )
    }
}

/// Thin HTTP/WebSocket wrapper. Marked `@unchecked Sendable` because its stored
/// `URLSession` and `Session` are themselves thread-safe.
private final class HTTPAPI: @unchecked Sendable {
    private let urlSession: URLSession
    private let session: Session

    init(session: Session) {
        self.session = session
        let configuration = URLSessionConfiguration.default
        configuration.waitsForConnectivity = true
        urlSession = URLSession(configuration: configuration)
    }

    func authenticate(path: String, baseURL: URL, username: String, password: String) async throws {
        let url = Self.join(baseURL, path)
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.httpBody = try JSONEncoder().encode(AuthBody(username: username, password: password))

        let (data, response) = try await urlSession.data(for: request)
        try Self.validate(response)
        let token = try JSONDecoder().decode(AuthResponse.self, from: data).token
        session.signIn(baseURL: baseURL, token: token)
    }

    func get<T: Decodable>(_ path: String, as type: T.Type) async throws -> T {
        let data = try await perform(path, method: "GET", body: Optional<EmptyBody>.none)
        return try JSONDecoder().decode(T.self, from: data)
    }

    @discardableResult
    func post(_ path: String) async throws -> Data {
        try await perform(path, method: "POST", body: Optional<EmptyBody>.none)
    }

    func post<B: Encodable>(_ path: String, body: B) async throws {
        _ = try await perform(path, method: "POST", body: body)
    }

    func post<B: Encodable, T: Decodable>(_ path: String, body: B, as type: T.Type) async throws -> T {
        let data = try await perform(path, method: "POST", body: body)
        return try JSONDecoder().decode(T.self, from: data)
    }

    func patch<B: Encodable>(_ path: String, body: B) async throws {
        _ = try await perform(path, method: "PATCH", body: body)
    }

    func delete(_ path: String) async throws {
        _ = try await perform(path, method: "DELETE", body: Optional<EmptyBody>.none)
    }

    /// Fetches raw bytes (file downloads) without JSON decoding.
    func getData(_ path: String) async throws -> Data {
        try await perform(path, method: "GET", body: Optional<EmptyBody>.none)
    }

    // MARK: File endpoint paths

    /// Listing / upload endpoint for a scope, with the directory as a `path`
    /// query parameter.
    func filesPath(_ scope: FileScope, query: String) -> String {
        "\(filesBase(scope))?path=\(Self.encodeQuery(query))"
    }

    /// Single-file endpoint (download / delete) for a scope, where the file path
    /// is part of the URL (`{*path}` wildcard).
    func filePath(_ scope: FileScope, path: String) -> String {
        "\(filesBase(scope))/\(Self.encodePath(path))"
    }

    /// Create-directory endpoint for a scope.
    func directoriesPath(_ scope: FileScope) -> String {
        switch scope {
        case .global:
            return "/api/files/directories"
        case let .workspace(threadID):
            return "/api/threads/\(threadID)/directories"
        }
    }

    private func filesBase(_ scope: FileScope) -> String {
        switch scope {
        case .global:
            return "/api/files"
        case let .workspace(threadID):
            return "/api/threads/\(threadID)/files"
        }
    }

    /// Uploads one or more files as `multipart/form-data`. The server accepts any
    /// field name, so each part is named `file`.
    func upload(_ path: String, files: [FileUpload]) async throws -> [UploadedFile] {
        guard let baseURL = session.baseURL else { throw FridayError.notConfigured }
        let boundary = "friday-\(UUID().uuidString)"
        var request = URLRequest(url: Self.join(baseURL, path))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("multipart/form-data; boundary=\(boundary)", forHTTPHeaderField: "Content-Type")
        if let token = session.token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        request.httpBody = Self.multipartBody(files: files, boundary: boundary)

        do {
            let (data, response) = try await urlSession.data(for: request)
            try Self.validate(response)
            return try JSONDecoder().decode(UploadResponse.self, from: data).files
        } catch let error as FridayError {
            throw error
        } catch {
            throw FridayError.transport
        }
    }

    private static func multipartBody(files: [FileUpload], boundary: String) -> Data {
        var body = Data()
        let newline = "\r\n"
        for file in files {
            body.append("--\(boundary)\(newline)")
            body.append("Content-Disposition: form-data; name=\"file\"; filename=\"\(file.name)\"\(newline)")
            body.append("Content-Type: \(file.mimeType ?? "application/octet-stream")\(newline)\(newline)")
            body.append(file.data)
            body.append(newline)
        }
        body.append("--\(boundary)--\(newline)")
        return body
    }

    /// Percent-encodes a URL path while preserving `/` separators.
    private static func encodePath(_ path: String) -> String {
        let trimmed = path.hasPrefix("/") ? String(path.dropFirst()) : path
        return trimmed.addingPercentEncoding(withAllowedCharacters: .urlPathAllowed) ?? trimmed
    }

    /// Percent-encodes a query-parameter value.
    private static func encodeQuery(_ value: String) -> String {
        var allowed = CharacterSet.urlQueryAllowed
        allowed.remove(charactersIn: "+&=?/")
        return value.addingPercentEncoding(withAllowedCharacters: allowed) ?? value
    }

    private func perform<B: Encodable>(_ path: String, method: String, body: B?) async throws -> Data {
        guard let baseURL = session.baseURL else { throw FridayError.notConfigured }
        var request = URLRequest(url: Self.join(baseURL, path))
        request.httpMethod = method
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        if let token = session.token {
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        }
        if let body {
            request.setValue("application/json", forHTTPHeaderField: "Content-Type")
            request.httpBody = try JSONEncoder().encode(body)
        }

        do {
            let (data, response) = try await urlSession.data(for: request)
            try Self.validate(response)
            return data
        } catch let error as FridayError {
            throw error
        } catch {
            throw FridayError.transport
        }
    }

    func eventStream(threadID: String) -> AsyncThrowingStream<ThreadEvent, Error> {
        AsyncThrowingStream { continuation in
            guard let baseURL = session.baseURL, let token = session.token else {
                continuation.finish(throwing: FridayError.notConfigured)
                return
            }

            var request = URLRequest(url: Self.webSocketURL(baseURL, threadID: threadID))
            request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
            let task = urlSession.webSocketTask(with: request)
            task.resume()

            let receiver = Task {
                do {
                    while true {
                        let message = try await task.receive()
                        if let event = Self.decodeEvent(message) {
                            continuation.yield(event)
                        }
                    }
                } catch {
                    continuation.finish(throwing: error)
                }
            }

            continuation.onTermination = { _ in
                receiver.cancel()
                task.cancel(with: .goingAway, reason: nil)
            }
        }
    }

    private static func decodeEvent(_ message: URLSessionWebSocketTask.Message) -> ThreadEvent? {
        let data: Data?
        switch message {
        case .string(let text):
            data = text.data(using: .utf8)
        case .data(let raw):
            data = raw
        @unknown default:
            data = nil
        }
        guard let data else { return nil }
        return try? JSONDecoder().decode(ThreadEvent.self, from: data)
    }

    private static func validate(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else { throw FridayError.transport }
        switch http.statusCode {
        case 200..<300:
            return
        case 401:
            throw FridayError.unauthorized
        default:
            throw FridayError.http(http.statusCode)
        }
    }

    private static func join(_ baseURL: URL, _ path: String) -> URL {
        var trimmed = baseURL.absoluteString
        if trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + path) ?? baseURL
    }

    private static func webSocketURL(_ baseURL: URL, threadID: String) -> URL {
        let scheme = (baseURL.scheme == "https") ? "wss" : "ws"
        let host = baseURL.host ?? ""
        let port = baseURL.port.map { ":\($0)" } ?? ""
        var prefix = baseURL.path
        if prefix.hasSuffix("/") { prefix.removeLast() }
        let string = "\(scheme)://\(host)\(port)\(prefix)/api/threads/\(threadID)/events"
        return URL(string: string) ?? baseURL
    }
}

private struct EmptyBody: Encodable {}

private struct AuthBody: Encodable {
    let username: String
    let password: String
}

private struct AuthResponse: Decodable {
    let token: String
}

private struct CreateThreadBody: Encodable {
    let content: String
    let projectID: String?
    let filePaths: [String]

    enum CodingKeys: String, CodingKey {
        case content
        case projectID = "project_id"
        case filePaths = "file_paths"
    }
}

private struct SendMessageBody: Encodable {
    let content: String
    let filePaths: [String]

    enum CodingKeys: String, CodingKey {
        case content
        case filePaths = "file_paths"
    }
}

private struct ApprovalBody: Encodable {
    let approved: Bool
}

private struct QuizAnswerBody: Encodable {
    let answers: [String]
}

private struct PathBody: Encodable {
    let path: String
}

private struct RenameBody: Encodable {
    let path: String
    let name: String
}

private struct UploadResponse: Decodable {
    let files: [UploadedFile]
}

private struct CreateAutomationBody: Encodable {
    let name: String
    let schedule: String
    let kind: String
    let payload: String
    let enabled: Bool
    let triggerKind: String
    let notifyKind: String
    let triggerConfig: [String: String]?

    init(_ automation: NewAutomation) {
        name = automation.name
        schedule = automation.schedule
        kind = automation.kind.rawValue
        payload = automation.payload
        enabled = automation.enabled
        triggerKind = automation.triggerKind.rawValue
        notifyKind = automation.notifyKind.rawValue
        triggerConfig = automation.triggerConfig
    }

    enum CodingKeys: String, CodingKey {
        case name, schedule, kind, payload, enabled
        case triggerKind = "trigger_kind"
        case notifyKind = "notify_kind"
        case triggerConfig = "trigger_config"
    }
}

private struct EnabledBody: Encodable {
    let enabled: Bool
}

private extension Data {
    mutating func append(_ string: String) {
        if let data = string.data(using: .utf8) { append(data) }
    }
}
