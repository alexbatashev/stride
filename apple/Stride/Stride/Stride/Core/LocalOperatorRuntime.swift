import Foundation

#if canImport(stride_operatorFFI)
import stride_operatorFFI
#endif

struct LocalOperatorResponse {
    let content: String
    let error: String?
}

struct LocalOperatorRuntimeEvent {
    let kind: Kind

    enum Kind {
        case agentDelta(String)
        case toolStarted(String)
        case toolFinished(String)
        case waitingForApproval(id: String, message: String)
        case approvalResolved(id: String, approved: Bool)
        case runFailed(String)
    }
}

protocol LocalOperatorThreadRuntime: AnyObject {
    var id: String { get }
    var title: String { get }

    func sendMessage(_ content: String, messages: [Message], session: Session) async throws -> LocalOperatorResponse
    func nextEvent(timeoutMs: UInt64) async -> LocalOperatorRuntimeEvent?
    func resolveApproval(id: String, approved: Bool) async -> Bool
}

struct LocalOperatorRuntimeFactory {
    func makeThread(initialContent: String, session: Session) async throws -> any LocalOperatorThreadRuntime {
        #if canImport(stride_operatorFFI)
        if let thread = RustLocalOperatorThread(initialContent: initialContent, session: session) {
            return thread
        }
        #endif

        return CloudCompatibleLocalOperatorThread(
            id: "local:\(UUID().uuidString)",
            title: Self.fallbackTitle(initialContent)
        )
    }

    static func fallbackTitle(_ content: String) -> String {
        let title = content
            .split(separator: " ")
            .prefix(8)
            .joined(separator: " ")
            .trimmingCharacters(in: .punctuationCharacters)
        return title.isEmpty ? "New local thread" : title
    }
}

#if canImport(stride_operatorFFI)
final class RustLocalOperatorThread: LocalOperatorThreadRuntime {
    private let handle: OperatorThreadHandle
    private let summaryValue: OperatorThreadSummary

    init?(initialContent: String, session: Session) {
        guard let baseURL = session.baseURL, let token = session.token else { return nil }

        let runtime = OperatorRuntime(
            cloudBaseUrl: baseURL.absoluteString,
            bearerToken: token,
            model: "default",
            workingDirectory: nil
        )
        handle = runtime.newThread()
        summaryValue = handle.summary()
    }

    var id: String { summaryValue.id }
    var title: String { summaryValue.title }

    func sendMessage(_ content: String, messages: [Message], session: Session) async throws -> LocalOperatorResponse {
        await Task.detached {
            let result = self.handle.sendMessage(content: content)
            return LocalOperatorResponse(content: result.content, error: result.error)
        }.value
    }

    func nextEvent(timeoutMs: UInt64) async -> LocalOperatorRuntimeEvent? {
        await Task.detached {
            guard let event = self.handle.nextEvent(timeoutMs: timeoutMs) else { return nil }
            return LocalOperatorRuntimeEvent(event)
        }.value
    }

    func resolveApproval(id: String, approved: Bool) async -> Bool {
        await Task.detached {
            self.handle.resolveApproval(approvalId: id, approved: approved)
        }.value
    }
}

extension LocalOperatorRuntimeEvent {
    #if canImport(stride_operatorFFI)
    init?(_ event: OperatorEvent) {
        switch event.kind {
        case "agent_delta":
            guard let content = event.content else { return nil }
            kind = .agentDelta(content)
        case "tool_started":
            guard let name = event.name else { return nil }
            kind = .toolStarted(name)
        case "tool_finished":
            guard let name = event.name else { return nil }
            kind = .toolFinished(name)
        case "waiting_for_approval":
            guard let id = event.approvalId, let message = event.message else { return nil }
            kind = .waitingForApproval(id: id, message: message)
        case "approval_resolved":
            guard let id = event.approvalId, let approved = event.approved else { return nil }
            kind = .approvalResolved(id: id, approved: approved)
        case "run_failed":
            kind = .runFailed(event.error ?? "Local operator failed.")
        default:
            return nil
        }
    }
    #endif
}
#endif

final class CloudCompatibleLocalOperatorThread: LocalOperatorThreadRuntime {
    private struct ChatRequest: Encodable {
        let model: String
        let messages: [ChatMessage]
    }

    private struct ChatMessage: Encodable {
        let role: String
        let content: String
    }

    private struct ChatResponse: Decodable {
        let choices: [Choice]

        struct Choice: Decodable {
            let message: ResponseMessage?
            let text: String?
        }

        struct ResponseMessage: Decodable {
            let content: String
        }
    }

    let id: String
    let title: String

    private let urlSession = URLSession(configuration: .default)

    init(id: String, title: String) {
        self.id = id
        self.title = title
    }

    func sendMessage(_ content: String, messages: [Message], session: Session) async throws -> LocalOperatorResponse {
        guard let baseURL = session.baseURL, let token = session.token else {
            throw StrideError.notConfigured
        }

        let requestBody = ChatRequest(
            model: "default",
            messages: messages.map { message in
                ChatMessage(role: openAIRole(message.role), content: message.content)
            }
        )

        var request = URLRequest(url: join(baseURL, "/v1/chat/completions"))
        request.httpMethod = "POST"
        request.setValue("application/json", forHTTPHeaderField: "Accept")
        request.setValue("application/json", forHTTPHeaderField: "Content-Type")
        request.setValue("Bearer \(token)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONEncoder().encode(requestBody)

        let (data, response) = try await urlSession.data(for: request)
        try validate(response)
        let decoded = try JSONDecoder().decode(ChatResponse.self, from: data)
        let content = decoded.choices.first?.message?.content ?? decoded.choices.first?.text ?? ""

        return LocalOperatorResponse(content: content, error: nil)
    }

    func nextEvent(timeoutMs: UInt64) async -> LocalOperatorRuntimeEvent? {
        nil
    }

    func resolveApproval(id: String, approved: Bool) async -> Bool {
        false
    }

    private func openAIRole(_ role: MessageRole) -> String {
        switch role {
        case .system:
            return "system"
        case .agent:
            return "assistant"
        case .user:
            return "user"
        case .tool:
            return "tool"
        }
    }

    private func validate(_ response: URLResponse) throws {
        guard let http = response as? HTTPURLResponse else { throw StrideError.transport }
        switch http.statusCode {
        case 200..<300:
            return
        case 401:
            throw StrideError.unauthorized
        default:
            throw StrideError.http(http.statusCode)
        }
    }

    private func join(_ baseURL: URL, _ path: String) -> URL {
        var trimmed = baseURL.absoluteString
        if trimmed.hasSuffix("/") { trimmed.removeLast() }
        return URL(string: trimmed + path) ?? baseURL
    }
}
