import CoreFridayMacros
import Fluent
import Foundation
import Observation

public enum TurnRole: String, Codable, CaseIterable, Sendable {
    case user
    case assistant
    case tool
    case system
}

@GenerateStoredModel(schema: "chat_threads")
@Observable
public final class ChatThread: Identifiable, Hashable, @unchecked Sendable {
    public var id: UUID
    // Owner user ID. Locally, it's ok for this value to be nil, but
    // the same model is used on the server, where this field is mandatory.
    public var userId: UUID?
    public var title: String
    public var createdAt: Date
    public var updatedAt: Date
    public var previewText: String
    public var isPinned: Bool

    public init(
        id: UUID = UUID(), userId: UUID?, title: String, createdAt: Date, updatedAt: Date,
        previewText: String, isPinned: Bool
    ) {
        self.id = id
        self.userId = userId
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
    }

    public static func == (lhs: ChatThread, rhs: ChatThread) -> Bool {
        lhs.id == rhs.id
    }

    public func hash(into hasher: inout Hasher) {
        hasher.combine(id)
    }
}

@GenerateStoredModel(schema: "chat_messages")
@Observable
public final class ChatMessage: Identifiable, @unchecked Sendable {
    public var id: UUID
    public var threadId: UUID
    public var userId: UUID?
    /// Parent message ID - support for branching/regeneration
    public var parentId: UUID?
    public var providerId: String
    public var modelId: String
    /// Human-readable model name
    public var modelName: String
    public var role: TurnRole
    public var thinking: String?
    public var content: String
    /// Contains JSON representation of tools call from model
    public var toolCall: String?
    /// Contains JSON representation of tool call results for model
    public var toolResult: String?
    public var createdAt: Date
    public var updatedAt: Date
    public var isDone: Bool
    /// JSON object with tokens/timings data
    public var usage: String?

    public init(
        id: UUID = UUID(),
        threadId: UUID = UUID(),
        userId: UUID? = nil,
        parentId: UUID? = nil,
        providerId: String = "",
        modelId: String = "",
        modelName: String = "",
        role: TurnRole,
        thinking: String? = nil,
        content: String,
        toolCall: String? = nil,
        toolResult: String? = nil,
        createdAt: Date = .now,
        updatedAt: Date = .now,
        isDone: Bool = false,
        usage: String? = nil
    ) {
        self.id = id
        self.threadId = threadId
        self.userId = userId
        self.parentId = parentId
        self.providerId = providerId
        self.modelId = modelId
        self.modelName = modelName
        self.role = role
        self.thinking = thinking
        self.content = content
        self.toolCall = toolCall
        self.toolResult = toolResult
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.isDone = isDone
        self.usage = usage
    }
}
