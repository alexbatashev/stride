import Foundation
import Fluent

public protocol ChatStorage: Sendable {
    func listMessages() async -> [ChatMessage]
    func appendMessage(message: ChatMessage) async
}

/// Intentionally no-op data storage for temporary in-memory chats
public struct NullChatStorage: ChatStorage {
    public func listMessages() async -> [ChatMessage] {
        []
    }

    public func appendMessage(message: ChatMessage) async {
        // intentionally nop
    }

    public init() {}
}

/// Purely in-memory chat storage for use only in tests
public actor MockChatStorage: ChatStorage {
    private var messages: [ChatMessage]

    public init(messages: [ChatMessage] = []) {
        self.messages = messages.sorted { $0.createdAt < $1.createdAt }
    }

    public func listMessages() async -> [ChatMessage] {
        messages
    }

    public func appendMessage(message: ChatMessage) async {
        messages.append(message)
    }
}

/// Fluent SQL-backed local database chat storage
public actor LocalChatStorage: ChatStorage {
    private let chatThreadId: UUID
    private let database: any Database

    public init(id: UUID, database: any Database) {
        self.chatThreadId = id
        self.database = database
    }

    public func listMessages() async -> [ChatMessage] {
        do {
            let storedMessages = try await StoredChatMessage.query(on: database)
                .filter(\.$threadId == chatThreadId)
                .sort(\.$createdAt, .ascending)
                .all()
                .get()

            return storedMessages.map {
                ChatMessage(
                    id: $0.id ?? UUID(),
                    threadId: $0.threadId,
                    userId: $0.userId,
                    parentId: $0.parentId,
                    providerId: $0.providerId,
                    modelId: $0.modelId,
                    modelName: $0.modelName,
                    role: $0.role,
                    thinking: $0.thinking,
                    content: $0.content,
                    toolCall: $0.toolCall,
                    toolResult: $0.toolResult,
                    createdAt: $0.createdAt,
                    updatedAt: $0.updatedAt,
                    isDone: $0.isDone,
                    usage: $0.usage
                )
            }
        } catch {
            return []
        }
    }

    public func appendMessage(message: ChatMessage) async {
        do {
            let now = Date.now
            let preview = message.content.trimmingCharacters(in: .whitespacesAndNewlines)

            if let thread = try await StoredChatThread.find(chatThreadId, on: database).get() {
                thread.updatedAt = max(thread.updatedAt, message.updatedAt)
                if !preview.isEmpty {
                    thread.previewText = preview
                }
                if thread.title.isEmpty {
                    thread.title = preview.isEmpty ? "Chat" : String(preview.prefix(80))
                }
                try await thread.update(on: database).get()
            } else {
                let thread = StoredChatThread()
                thread.id = chatThreadId
                thread.userId = message.userId
                thread.title = preview.isEmpty ? "Chat" : String(preview.prefix(80))
                thread.createdAt = message.createdAt
                thread.updatedAt = message.updatedAt
                thread.previewText = preview
                thread.isPinned = false
                try await thread.create(on: database).get()
            }

            let storedMessage = StoredChatMessage()
            storedMessage.id = message.id
            storedMessage.threadId = chatThreadId
            storedMessage.userId = message.userId
            storedMessage.parentId = message.parentId
            storedMessage.providerId = message.providerId
            storedMessage.modelId = message.modelId
            storedMessage.modelName = message.modelName
            storedMessage.role = message.role
            storedMessage.thinking = message.thinking
            storedMessage.content = message.content
            storedMessage.toolCall = message.toolCall
            storedMessage.toolResult = message.toolResult
            storedMessage.createdAt = message.createdAt
            storedMessage.updatedAt = message.updatedAt == message.createdAt ? now : message.updatedAt
            storedMessage.isDone = message.isDone
            storedMessage.usage = message.usage
            try await storedMessage.create(on: database).get()
        } catch {
            // Intentionally best-effort: chat should continue even if persistence fails.
        }
    }
}
