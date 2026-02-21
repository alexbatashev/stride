import Foundation
import SwiftData

// MARK: - Domain Enums

enum TurnRole: String, Codable, CaseIterable, Sendable {
    case user
    case assistant
    case tool
    case system
}

enum AttachmentKind: String, Codable, CaseIterable, Sendable {
    case image
    case file
    case audio
    case video
}

enum ToolInvocationStatus: String, Codable, CaseIterable, Sendable {
    case queued
    case running
    case completed
    case failed
}

// MARK: - SwiftData Models

@Model
final class Conversation {
    @Attribute(.unique) var id: UUID
    var title: String
    var createdAt: Date
    var updatedAt: Date
    var previewText: String
    var isPinned: Bool

    var turns: [ConversationTurn] = []

    init(
        id: UUID = UUID(),
        title: String = "New Chat",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        previewText: String = "",
        isPinned: Bool = false
    ) {
        self.id = id
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
    }

    var orderedTurns: [ConversationTurn] {
        turns.sorted {
            if $0.sequenceNumber == $1.sequenceNumber {
                return $0.createdAt < $1.createdAt
            }
            return $0.sequenceNumber < $1.sequenceNumber
        }
    }

    var nextSequenceNumber: Int {
        (turns.map(\.sequenceNumber).max() ?? -1) + 1
    }

    func refreshPreview(using message: String) {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        previewText = String(trimmed.prefix(80))
        updatedAt = .now

        if title == "New Chat", !trimmed.isEmpty {
            title = String(trimmed.prefix(36))
        }
    }
}

@Model
final class ConversationTurn {
    @Attribute(.unique) var id: UUID
    var roleRawValue: String
    var text: String
    var createdAt: Date
    var sequenceNumber: Int
    var modelIdentifier: String?
    var isError: Bool

    var conversation: Conversation?

    var attachments: [TurnAttachment] = []

    var toolInvocations: [ToolInvocation] = []

    init(
        id: UUID = UUID(),
        role: TurnRole,
        text: String,
        createdAt: Date = .now,
        sequenceNumber: Int,
        modelIdentifier: String? = nil,
        isError: Bool = false,
        conversation: Conversation? = nil
    ) {
        self.id = id
        self.roleRawValue = role.rawValue
        self.text = text
        self.createdAt = createdAt
        self.sequenceNumber = sequenceNumber
        self.modelIdentifier = modelIdentifier
        self.isError = isError
        self.conversation = conversation
    }

    var role: TurnRole {
        get { TurnRole(rawValue: roleRawValue) ?? .assistant }
        set { roleRawValue = newValue.rawValue }
    }
}

@Model
final class TurnAttachment {
    @Attribute(.unique) var id: UUID
    var kindRawValue: String
    var fileName: String
    var mimeType: String
    var localPath: String
    var byteCount: Int
    var createdAt: Date

    var turn: ConversationTurn?

    init(
        id: UUID = UUID(),
        kind: AttachmentKind,
        fileName: String,
        mimeType: String,
        localPath: String,
        byteCount: Int,
        createdAt: Date = .now,
        turn: ConversationTurn? = nil
    ) {
        self.id = id
        self.kindRawValue = kind.rawValue
        self.fileName = fileName
        self.mimeType = mimeType
        self.localPath = localPath
        self.byteCount = byteCount
        self.createdAt = createdAt
        self.turn = turn
    }

    var kind: AttachmentKind {
        get { AttachmentKind(rawValue: kindRawValue) ?? .file }
        set { kindRawValue = newValue.rawValue }
    }
}

@Model
final class ToolInvocation {
    @Attribute(.unique) var id: UUID
    var name: String
    var argumentsJSON: String
    var resultJSON: String?
    var statusRawValue: String
    var startedAt: Date
    var endedAt: Date?

    var turn: ConversationTurn?

    init(
        id: UUID = UUID(),
        name: String,
        argumentsJSON: String,
        resultJSON: String? = nil,
        status: ToolInvocationStatus,
        startedAt: Date = .now,
        endedAt: Date? = nil,
        turn: ConversationTurn? = nil
    ) {
        self.id = id
        self.name = name
        self.argumentsJSON = argumentsJSON
        self.resultJSON = resultJSON
        self.statusRawValue = status.rawValue
        self.startedAt = startedAt
        self.endedAt = endedAt
        self.turn = turn
    }

    var status: ToolInvocationStatus {
        get { ToolInvocationStatus(rawValue: statusRawValue) ?? .queued }
        set { statusRawValue = newValue.rawValue }
    }
}
