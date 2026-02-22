import Fluent
import FluentSQLiteDriver
import Foundation
import Logging
import NIOPosix

public struct CoreFridaySnapshot: Sendable {
    public var conversations: [Conversation]
    public var notes: [Note]
    public var chatSettings: ChatSettingsPersistedState?

    public init(
        conversations: [Conversation],
        notes: [Note],
        chatSettings: ChatSettingsPersistedState?
    ) {
        self.conversations = conversations
        self.notes = notes
        self.chatSettings = chatSettings
    }
}

public final class CoreFridayStorage: @unchecked Sendable {
    private let eventLoopGroup: MultiThreadedEventLoopGroup
    private let threadPool: NIOThreadPool
    private let databases: Databases
    private let logger: Logger

    private var database: Database {
        guard let db = databases.database(logger: logger, on: eventLoopGroup.next()) else {
            fatalError("Database is not configured")
        }
        return db
    }

    public init(databaseFilePath: String) throws {
        eventLoopGroup = MultiThreadedEventLoopGroup(numberOfThreads: 1)
        threadPool = NIOThreadPool(numberOfThreads: 1)
        threadPool.start()
        databases = Databases(threadPool: threadPool, on: eventLoopGroup)

        logger = Logger(label: "CoreFridayStorage")

        databases.use(.sqlite(.file(databaseFilePath)), as: .sqlite)
        databases.default(to: .sqlite)

        try runMigrationsIfNeeded()
    }

    deinit {
        do {
            try databases.shutdown()
            try threadPool.syncShutdownGracefully()
            try eventLoopGroup.syncShutdownGracefully()
        } catch {
            // Best-effort shutdown during deinit.
        }
    }

    public func loadSnapshot() throws -> CoreFridaySnapshot {
        let storedConversations = try StoredConversation.query(on: database)
            .sort(\.$updatedAt, .descending)
            .all()
            .wait()
        let storedTurns = try StoredConversationTurn.query(on: database).all().wait()

        let storedNotes = try StoredNote.query(on: database)
            .sort(\.$updatedAt, .descending)
            .all()
            .wait()
        let storedBlocks = try StoredNoteBlock.query(on: database).all().wait()

        let settings = try StoredChatSettings.query(on: database).first().wait()

        let turnsByConversation = Dictionary(grouping: storedTurns, by: \.conversationDomainID)
        let blocksByNote = Dictionary(grouping: storedBlocks, by: \.noteDomainID)

        let conversations = try storedConversations.map { stored in
            let turns = try (turnsByConversation[stored.domainID] ?? [])
                .map(Self.mapConversationTurn)

            return Conversation(
                id: UUID(uuidString: stored.domainID) ?? UUID(),
                title: stored.title,
                createdAt: stored.createdAt,
                updatedAt: stored.updatedAt,
                previewText: stored.previewText,
                isPinned: stored.isPinned,
                turns: turns
            )
        }

        let notes = try storedNotes.map { stored in
            let blocks = try (blocksByNote[stored.domainID] ?? [])
                .map(Self.mapNoteBlock)

            return Note(
                id: UUID(uuidString: stored.domainID) ?? UUID(),
                title: stored.title,
                createdAt: stored.createdAt,
                updatedAt: stored.updatedAt,
                previewText: stored.previewText,
                isPinned: stored.isPinned,
                blocks: blocks
            )
        }

        let chatSettings: ChatSettingsPersistedState?
        if let settings {
            chatSettings = try JSONDecoder().decode(ChatSettingsPersistedState.self, from: Data(settings.payloadUTF8.utf8))
        } else {
            chatSettings = nil
        }

        return CoreFridaySnapshot(
            conversations: conversations,
            notes: notes,
            chatSettings: chatSettings
        )
    }

    public func replaceSnapshot(
        conversations: [Conversation],
        notes: [Note],
        chatSettings: ChatSettingsPersistedState
    ) throws {
        let db = database

        try StoredConversationTurn.query(on: db).delete().wait()
        try StoredConversation.query(on: db).delete().wait()

        try StoredNoteBlock.query(on: db).delete().wait()
        try StoredNote.query(on: db).delete().wait()

        try StoredChatSettings.query(on: db).delete().wait()

        for conversation in conversations {
            let storedConversation = StoredConversation(
                domainID: conversation.id.uuidString,
                title: conversation.title,
                createdAt: conversation.createdAt,
                updatedAt: conversation.updatedAt,
                previewText: conversation.previewText,
                isPinned: conversation.isPinned
            )
            try storedConversation.create(on: db).wait()

            for turn in conversation.turns {
                let attachmentsData = try JSONEncoder().encode(turn.attachments)
                let toolsData = try JSONEncoder().encode(turn.toolInvocations)

                let storedTurn = StoredConversationTurn(
                    domainID: turn.id.uuidString,
                    conversationDomainID: conversation.id.uuidString,
                    role: turn.role.rawValue,
                    text: turn.text,
                    createdAt: turn.createdAt,
                    sequenceNumber: turn.sequenceNumber,
                    modelIdentifier: turn.modelIdentifier,
                    isError: turn.isError,
                    attachmentsJSON: String(decoding: attachmentsData, as: UTF8.self),
                    toolInvocationsJSON: String(decoding: toolsData, as: UTF8.self)
                )

                try storedTurn.create(on: db).wait()
            }
        }

        for note in notes {
            let storedNote = StoredNote(
                domainID: note.id.uuidString,
                title: note.title,
                createdAt: note.createdAt,
                updatedAt: note.updatedAt,
                previewText: note.previewText,
                isPinned: note.isPinned
            )
            try storedNote.create(on: db).wait()

            for block in note.blocks {
                let attachmentsData = try JSONEncoder().encode(block.attachments)

                let storedBlock = StoredNoteBlock(
                    domainID: block.id.uuidString,
                    noteDomainID: note.id.uuidString,
                    kind: block.kind.rawValue,
                    orderIndex: block.orderIndex,
                    textContent: block.textContent,
                    payloadJSON: block.payloadJSON,
                    createdAt: block.createdAt,
                    updatedAt: block.updatedAt,
                    attachmentsJSON: String(decoding: attachmentsData, as: UTF8.self)
                )

                try storedBlock.create(on: db).wait()
            }
        }

        let chatData = try JSONEncoder().encode(chatSettings)
        let storedSettings = StoredChatSettings(payloadUTF8: String(decoding: chatData, as: UTF8.self))
        try storedSettings.create(on: db).wait()
    }

    private func runMigrationsIfNeeded() throws {
        try CreateStoredConversation().prepare(on: database).wait()
        try CreateStoredConversationTurn().prepare(on: database).wait()
        try CreateStoredNote().prepare(on: database).wait()
        try CreateStoredNoteBlock().prepare(on: database).wait()
        try CreateStoredChatSettings().prepare(on: database).wait()
    }

    private static func mapConversationTurn(_ stored: StoredConversationTurn) throws -> ConversationTurn {
        let attachments = try JSONDecoder().decode([TurnAttachment].self, from: Data(stored.attachmentsJSON.utf8))
        let tools = try JSONDecoder().decode([ToolInvocation].self, from: Data(stored.toolInvocationsJSON.utf8))

        return ConversationTurn(
            id: UUID(uuidString: stored.domainID) ?? UUID(),
            role: TurnRole(rawValue: stored.role) ?? .assistant,
            text: stored.text,
            createdAt: stored.createdAt,
            sequenceNumber: stored.sequenceNumber,
            modelIdentifier: stored.modelIdentifier,
            isError: stored.isError,
            attachments: attachments,
            toolInvocations: tools
        )
    }

    private static func mapNoteBlock(_ stored: StoredNoteBlock) throws -> NoteBlock {
        let attachments = try JSONDecoder().decode([NoteAttachment].self, from: Data(stored.attachmentsJSON.utf8))

        return NoteBlock(
            id: UUID(uuidString: stored.domainID) ?? UUID(),
            kind: NoteBlockKind(rawValue: stored.kind) ?? .text,
            orderIndex: stored.orderIndex,
            textContent: stored.textContent,
            payloadJSON: stored.payloadJSON,
            createdAt: stored.createdAt,
            updatedAt: stored.updatedAt,
            attachments: attachments
        )
    }
}

private final class StoredConversation: Model, @unchecked Sendable {
    static let schema = "conversations"

    @ID(key: .id)
    var id: UUID?

    @Field(key: "domain_id")
    var domainID: String

    @Field(key: "title")
    var title: String

    @Field(key: "created_at")
    var createdAt: Date

    @Field(key: "updated_at")
    var updatedAt: Date

    @Field(key: "preview_text")
    var previewText: String

    @Field(key: "is_pinned")
    var isPinned: Bool

    init() {}

    init(
        domainID: String,
        title: String,
        createdAt: Date,
        updatedAt: Date,
        previewText: String,
        isPinned: Bool
    ) {
        self.domainID = domainID
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
    }
}

private struct CreateStoredConversation: Migration {
    func prepare(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredConversation.schema)
            .id()
            .field("domain_id", .string, .required)
            .field("title", .string, .required)
            .field("created_at", .datetime, .required)
            .field("updated_at", .datetime, .required)
            .field("preview_text", .string, .required)
            .field("is_pinned", .bool, .required)
            .unique(on: "domain_id")
            .ignoreExisting()
            .create()
    }

    func revert(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredConversation.schema).delete()
    }
}

private final class StoredConversationTurn: Model, @unchecked Sendable {
    static let schema = "conversation_turns"

    @ID(key: .id)
    var id: UUID?

    @Field(key: "domain_id")
    var domainID: String

    @Field(key: "conversation_domain_id")
    var conversationDomainID: String

    @Field(key: "role")
    var role: String

    @Field(key: "text")
    var text: String

    @Field(key: "created_at")
    var createdAt: Date

    @Field(key: "sequence_number")
    var sequenceNumber: Int

    @OptionalField(key: "model_identifier")
    var modelIdentifier: String?

    @Field(key: "is_error")
    var isError: Bool

    @Field(key: "attachments_json")
    var attachmentsJSON: String

    @Field(key: "tool_invocations_json")
    var toolInvocationsJSON: String

    init() {}

    init(
        domainID: String,
        conversationDomainID: String,
        role: String,
        text: String,
        createdAt: Date,
        sequenceNumber: Int,
        modelIdentifier: String?,
        isError: Bool,
        attachmentsJSON: String,
        toolInvocationsJSON: String
    ) {
        self.domainID = domainID
        self.conversationDomainID = conversationDomainID
        self.role = role
        self.text = text
        self.createdAt = createdAt
        self.sequenceNumber = sequenceNumber
        self.modelIdentifier = modelIdentifier
        self.isError = isError
        self.attachmentsJSON = attachmentsJSON
        self.toolInvocationsJSON = toolInvocationsJSON
    }
}

private struct CreateStoredConversationTurn: Migration {
    func prepare(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredConversationTurn.schema)
            .id()
            .field("domain_id", .string, .required)
            .field("conversation_domain_id", .string, .required)
            .field("role", .string, .required)
            .field("text", .string, .required)
            .field("created_at", .datetime, .required)
            .field("sequence_number", .int, .required)
            .field("model_identifier", .string)
            .field("is_error", .bool, .required)
            .field("attachments_json", .string, .required)
            .field("tool_invocations_json", .string, .required)
            .unique(on: "domain_id")
            .ignoreExisting()
            .create()
    }

    func revert(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredConversationTurn.schema).delete()
    }
}

private final class StoredNote: Model, @unchecked Sendable {
    static let schema = "notes"

    @ID(key: .id)
    var id: UUID?

    @Field(key: "domain_id")
    var domainID: String

    @Field(key: "title")
    var title: String

    @Field(key: "created_at")
    var createdAt: Date

    @Field(key: "updated_at")
    var updatedAt: Date

    @Field(key: "preview_text")
    var previewText: String

    @Field(key: "is_pinned")
    var isPinned: Bool

    init() {}

    init(
        domainID: String,
        title: String,
        createdAt: Date,
        updatedAt: Date,
        previewText: String,
        isPinned: Bool
    ) {
        self.domainID = domainID
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
    }
}

private struct CreateStoredNote: Migration {
    func prepare(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredNote.schema)
            .id()
            .field("domain_id", .string, .required)
            .field("title", .string, .required)
            .field("created_at", .datetime, .required)
            .field("updated_at", .datetime, .required)
            .field("preview_text", .string, .required)
            .field("is_pinned", .bool, .required)
            .unique(on: "domain_id")
            .ignoreExisting()
            .create()
    }

    func revert(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredNote.schema).delete()
    }
}

private final class StoredNoteBlock: Model, @unchecked Sendable {
    static let schema = "note_blocks"

    @ID(key: .id)
    var id: UUID?

    @Field(key: "domain_id")
    var domainID: String

    @Field(key: "note_domain_id")
    var noteDomainID: String

    @Field(key: "kind")
    var kind: String

    @Field(key: "order_index")
    var orderIndex: Int

    @Field(key: "text_content")
    var textContent: String

    @Field(key: "payload_json")
    var payloadJSON: String

    @Field(key: "created_at")
    var createdAt: Date

    @Field(key: "updated_at")
    var updatedAt: Date

    @Field(key: "attachments_json")
    var attachmentsJSON: String

    init() {}

    init(
        domainID: String,
        noteDomainID: String,
        kind: String,
        orderIndex: Int,
        textContent: String,
        payloadJSON: String,
        createdAt: Date,
        updatedAt: Date,
        attachmentsJSON: String
    ) {
        self.domainID = domainID
        self.noteDomainID = noteDomainID
        self.kind = kind
        self.orderIndex = orderIndex
        self.textContent = textContent
        self.payloadJSON = payloadJSON
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.attachmentsJSON = attachmentsJSON
    }
}

private struct CreateStoredNoteBlock: Migration {
    func prepare(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredNoteBlock.schema)
            .id()
            .field("domain_id", .string, .required)
            .field("note_domain_id", .string, .required)
            .field("kind", .string, .required)
            .field("order_index", .int, .required)
            .field("text_content", .string, .required)
            .field("payload_json", .string, .required)
            .field("created_at", .datetime, .required)
            .field("updated_at", .datetime, .required)
            .field("attachments_json", .string, .required)
            .unique(on: "domain_id")
            .ignoreExisting()
            .create()
    }

    func revert(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredNoteBlock.schema).delete()
    }
}

private final class StoredChatSettings: Model, @unchecked Sendable {
    static let schema = "chat_settings"

    @ID(key: .id)
    var id: UUID?

    @Field(key: "payload_utf8")
    var payloadUTF8: String

    init() {}

    init(payloadUTF8: String) {
        self.payloadUTF8 = payloadUTF8
    }
}

private struct CreateStoredChatSettings: Migration {
    func prepare(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatSettings.schema)
            .id()
            .field("payload_utf8", .string, .required)
            .ignoreExisting()
            .create()
    }

    func revert(on database: Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatSettings.schema).delete()
    }
}
