import Foundation

public struct ConversationTurnDTO: Codable, Sendable {
    public var id: UUID
    public var role: TurnRole
    public var text: String
    public var createdAt: Date
    public var modelIdentifier: String?
    public var isError: Bool
    public var attachments: [TurnAttachment]
    public var toolInvocations: [ToolInvocation]

    public init(
        id: UUID,
        role: TurnRole,
        text: String,
        createdAt: Date,
        modelIdentifier: String?,
        isError: Bool,
        attachments: [TurnAttachment],
        toolInvocations: [ToolInvocation]
    ) {
        self.id = id
        self.role = role
        self.text = text
        self.createdAt = createdAt
        self.modelIdentifier = modelIdentifier
        self.isError = isError
        self.attachments = attachments
        self.toolInvocations = toolInvocations
    }
}

public struct ConversationDTO: Codable, Sendable {
    public var id: UUID
    public var title: String
    public var createdAt: Date
    public var updatedAt: Date
    public var previewText: String
    public var isPinned: Bool
    public var turns: [ConversationTurnDTO]

    public init(
        id: UUID,
        title: String,
        createdAt: Date,
        updatedAt: Date,
        previewText: String,
        isPinned: Bool,
        turns: [ConversationTurnDTO] = []
    ) {
        self.id = id
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
        self.turns = turns
    }
}

public struct CreateConversationRequest: Codable, Sendable {
    public var id: UUID?
    public var title: String
    public var isPinned: Bool

    public init(id: UUID? = nil, title: String, isPinned: Bool = false) {
        self.id = id
        self.title = title
        self.isPinned = isPinned
    }
}
