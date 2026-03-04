import Foundation

// MARK: - ChatThread

extension ChatThread: Identifiable {}

extension ChatThread {
    public var createdAt: Date {
        Date(timeIntervalSince1970: Double(createdAtMs) / 1000)
    }

    public var updatedAt: Date {
        Date(timeIntervalSince1970: Double(updatedAtMs) / 1000)
    }

    public func withPreviewText(_ text: String, updatedAt: Date = .now) -> ChatThread {
        ChatThread(
            id: id,
            userId: userId,
            title: title,
            createdAtMs: createdAtMs,
            updatedAtMs: Int64(updatedAt.timeIntervalSince1970 * 1000),
            previewText: text,
            isPinned: isPinned
        )
    }
}

// MARK: - ChatMessage

extension ChatMessage: Identifiable {}

extension ChatMessage {
    public var createdAt: Date {
        Date(timeIntervalSince1970: Double(createdAtMs) / 1000)
    }

    public static func makeNew(threadId: String, role: TurnRole, content: String) -> ChatMessage {
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        return ChatMessage(
            id: UUID().uuidString,
            threadId: threadId,
            userId: nil,
            parentId: nil,
            providerId: "",
            modelId: "",
            modelName: "",
            role: role,
            thinking: nil,
            content: content,
            toolCall: nil,
            toolResult: nil,
            createdAtMs: now,
            updatedAtMs: now,
            isDone: false,
            usage: nil
        )
    }

    public func withContent(_ content: String) -> ChatMessage {
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        return ChatMessage(
            id: id,
            threadId: threadId,
            userId: userId,
            parentId: parentId,
            providerId: providerId,
            modelId: modelId,
            modelName: modelName,
            role: role,
            thinking: thinking,
            content: content,
            toolCall: toolCall,
            toolResult: toolResult,
            createdAtMs: createdAtMs,
            updatedAtMs: now,
            isDone: isDone,
            usage: usage
        )
    }
}

// MARK: - LangModel

extension LangModel {
    public func readableName() -> String {
        if modelName.isEmpty || modelName == model {
            return model
        }
        return "\(providerName) / \(modelName)"
    }
}

// MARK: - ChatProviderConfiguration

extension ChatProviderConfiguration: Identifiable {}

extension ChatProviderConfiguration {
    public var baseURL: String { baseUrl }
}

// MARK: - ChatProviderKind

extension ChatProviderKind: CaseIterable {
    public static var allCases: [ChatProviderKind] {
        [.openAiCompatible, .ollama, .anthropic, .mock]
    }
}

extension ChatProviderKind: Identifiable {
    public var id: String {
        switch self {
        case .openAiCompatible: return "openAICompatible"
        case .ollama: return "ollama"
        case .anthropic: return "anthropic"
        case .mock: return "mock"
        }
    }
}

extension ChatProviderKind {
    public var displayName: String {
        switch self {
        case .openAiCompatible: return "OpenAI-Compatible"
        case .ollama: return "Ollama"
        case .anthropic: return "Anthropic"
        case .mock: return "Mock"
        }
    }
}

// MARK: - ChatService

extension ChatService {
    public func addMessage(toolsEnabled: Bool = false, next: ChatMessage) -> AsyncThrowingStream<ChatMessage, Error> {
        AsyncThrowingStream { continuation in
            Task {
                do {
                    let chunks = try await self.addMessageCollect(toolsEnabled: toolsEnabled, next: next)
                    for chunk in chunks {
                        continuation.yield(chunk)
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }
}
