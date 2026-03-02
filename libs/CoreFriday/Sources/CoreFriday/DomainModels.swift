import Foundation
import Observation

public enum TurnRole: String, Codable, CaseIterable, Sendable {
    case user
    case assistant
    case tool
    case system
}

public enum AttachmentKind: String, Codable, CaseIterable, Sendable {
    case image
    case file
    case audio
    case video
}

public enum ToolInvocationStatus: String, Codable, CaseIterable, Sendable {
    case queued
    case running
    case completed
    case failed
}

public struct TurnAttachment: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID
    public var kind: AttachmentKind
    public var fileName: String
    public var mimeType: String
    public var localPath: String
    public var byteCount: Int
    public var createdAt: Date

    public init(
        id: UUID = UUID(),
        kind: AttachmentKind,
        fileName: String,
        mimeType: String,
        localPath: String,
        byteCount: Int,
        createdAt: Date = .now
    ) {
        self.id = id
        self.kind = kind
        self.fileName = fileName
        self.mimeType = mimeType
        self.localPath = localPath
        self.byteCount = byteCount
        self.createdAt = createdAt
    }
}

public struct ToolInvocation: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var argumentsJSON: String
    public var resultJSON: String?
    public var status: ToolInvocationStatus
    public var startedAt: Date
    public var endedAt: Date?

    public init(
        id: UUID = UUID(),
        name: String,
        argumentsJSON: String,
        resultJSON: String? = nil,
        status: ToolInvocationStatus,
        startedAt: Date = .now,
        endedAt: Date? = nil
    ) {
        self.id = id
        self.name = name
        self.argumentsJSON = argumentsJSON
        self.resultJSON = resultJSON
        self.status = status
        self.startedAt = startedAt
        self.endedAt = endedAt
    }
}

@Observable
public final class ConversationTurn: Identifiable, @unchecked Sendable {
    public var id: UUID
    public var role: TurnRole
    public var text: String
    public var createdAt: Date
    public var modelIdentifier: String?
    public var isError: Bool
    public var attachments: [TurnAttachment]
    public var toolInvocations: [ToolInvocation]

    public init(
        id: UUID = UUID(),
        role: TurnRole,
        text: String,
        createdAt: Date = .now,
        modelIdentifier: String? = nil,
        isError: Bool = false,
        attachments: [TurnAttachment] = [],
        toolInvocations: [ToolInvocation] = []
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

@Observable
public final class Conversation: Identifiable, Hashable, @unchecked Sendable {
    public var id: UUID
    public var title: String
    public var createdAt: Date
    public var updatedAt: Date
    public var previewText: String
    public var isPinned: Bool
    public var turns: [ConversationTurn]

    public init(
        id: UUID = UUID(),
        title: String = "New Chat",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        previewText: String = "",
        isPinned: Bool = false,
        turns: [ConversationTurn] = []
    ) {
        self.id = id
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
        self.turns = turns
    }

    public var orderedTurns: [ConversationTurn] {
        turns.sorted { $0.createdAt < $1.createdAt }
    }

    public static func == (lhs: Conversation, rhs: Conversation) -> Bool { lhs.id == rhs.id }
    public func hash(into hasher: inout Hasher) { hasher.combine(id) }

    public func refreshPreview(using message: String) {
        let trimmed = message.trimmingCharacters(in: .whitespacesAndNewlines)
        previewText = String(trimmed.prefix(80))
        updatedAt = .now

        if title == "New Chat", !trimmed.isEmpty {
            title = String(trimmed.prefix(36))
        }
    }
}

public enum NoteBlockKind: String, Codable, CaseIterable, Sendable {
    case text
    case heading
    case checklist
    case table
    case image
    case drawing
    case attachment
    case code
    case quote
}

public enum NoteAttachmentKind: String, Codable, CaseIterable, Sendable {
    case image
    case drawing
    case file
    case audio
    case video
}

public struct NoteAttachment: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID
    public var kind: NoteAttachmentKind
    public var fileName: String
    public var mimeType: String
    public var localPath: String
    public var byteCount: Int
    public var metadataJSON: String
    public var createdAt: Date

    public init(
        id: UUID = UUID(),
        kind: NoteAttachmentKind,
        fileName: String,
        mimeType: String,
        localPath: String,
        byteCount: Int,
        metadataJSON: String = "{}",
        createdAt: Date = .now
    ) {
        self.id = id
        self.kind = kind
        self.fileName = fileName
        self.mimeType = mimeType
        self.localPath = localPath
        self.byteCount = byteCount
        self.metadataJSON = metadataJSON
        self.createdAt = createdAt
    }
}

@Observable
public final class NoteBlock: Identifiable, @unchecked Sendable {
    public var id: UUID
    public var kind: NoteBlockKind
    public var orderIndex: Int
    public var textContent: String
    public var payloadJSON: String
    public var createdAt: Date
    public var updatedAt: Date
    public var attachments: [NoteAttachment]

    public init(
        id: UUID = UUID(),
        kind: NoteBlockKind,
        orderIndex: Int,
        textContent: String = "",
        payloadJSON: String = "{}",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        attachments: [NoteAttachment] = []
    ) {
        self.id = id
        self.kind = kind
        self.orderIndex = orderIndex
        self.textContent = textContent
        self.payloadJSON = payloadJSON
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.attachments = attachments
    }

    public var plainTextPreview: String? {
        let trimmed = textContent.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }

        switch kind {
        case .table:
            return "Table"
        case .image:
            return "Image"
        case .drawing:
            return "Drawing"
        case .attachment:
            return "Attachment"
        case .checklist:
            return "Checklist"
        case .text, .heading, .code, .quote:
            return nil
        }
    }
}

@Observable
public final class Note: Identifiable, Hashable, @unchecked Sendable {
    public var id: UUID
    public var title: String
    public var createdAt: Date
    public var updatedAt: Date
    public var previewText: String
    public var isPinned: Bool
    public var blocks: [NoteBlock]

    public init(
        id: UUID = UUID(),
        title: String = "New Note",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        previewText: String = "",
        isPinned: Bool = false,
        blocks: [NoteBlock] = []
    ) {
        self.id = id
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
        self.blocks = blocks
    }

    public static func == (lhs: Note, rhs: Note) -> Bool { lhs.id == rhs.id }
    public func hash(into hasher: inout Hasher) { hasher.combine(id) }

    public var orderedBlocks: [NoteBlock] {
        blocks.sorted {
            if $0.orderIndex == $1.orderIndex {
                return $0.createdAt < $1.createdAt
            }
            return $0.orderIndex < $1.orderIndex
        }
    }

    public var nextOrderIndex: Int {
        (blocks.map(\.orderIndex).max() ?? -1) + 1
    }

    public func refreshPreview() {
        let firstPreview = orderedBlocks
            .lazy
            .compactMap(\.plainTextPreview)
            .first(where: { !$0.isEmpty }) ?? ""

        previewText = String(firstPreview.prefix(100))
        updatedAt = .now

        let trimmedPreview = firstPreview.trimmingCharacters(in: .whitespacesAndNewlines)
        if title == "New Note", !trimmedPreview.isEmpty {
            title = String(trimmedPreview.prefix(36))
        }
    }
}

public enum ChatProviderKind: String, Codable, CaseIterable, Identifiable, Sendable {
    case openAICompatible
    case ollama
    case anthropic
    case mock

    public var id: String { rawValue }

    public var displayName: String {
        switch self {
        case .openAICompatible:
            return "OpenAI-Compatible"
        case .ollama:
            return "Ollama"
        case .anthropic:
            return "Anthropic"
        case .mock:
            return "Mock"
        }
    }
}

public struct ChatProviderConfiguration: Codable, Identifiable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var kind: ChatProviderKind
    public var baseURL: String
    public var token: String
    public var defaultModel: String

    public init(
        id: UUID = UUID(),
        name: String,
        kind: ChatProviderKind,
        baseURL: String,
        token: String = "",
        defaultModel: String = ""
    ) {
        self.id = id
        self.name = name
        self.kind = kind
        self.baseURL = baseURL
        self.token = token
        self.defaultModel = defaultModel
    }

    public static func starterOpenAI() -> Self {
        .init(
            name: "OpenAI",
            kind: .openAICompatible,
            baseURL: "https://api.openai.com",
            token: "",
            defaultModel: "gpt-4.1"
        )
    }

    public static func starterOllama() -> Self {
        .init(
            name: "Local Ollama",
            kind: .ollama,
            baseURL: "http://localhost:11434",
            token: "",
            defaultModel: ""
        )
    }
}

public struct ChatSettingsPersistedState: Codable, Sendable {
    public var providers: [ChatProviderConfiguration]
    public var selectedProviderID: UUID?
    public var selectedModel: String

    public init(
        providers: [ChatProviderConfiguration],
        selectedProviderID: UUID?,
        selectedModel: String
    ) {
        self.providers = providers
        self.selectedProviderID = selectedProviderID
        self.selectedModel = selectedModel
    }
}

@MainActor
@Observable
public final class ChatProviderSettingsStore {
    public var providers: [ChatProviderConfiguration]
    public var selectedProviderID: UUID?
    public var selectedModel: String
    public var availableModels: [String] = []
    public var isRefreshingModels = false
    public var refreshErrorMessage: String?

    public var onChange: (() -> Void)?

    private let fallbackProviders: [ChatProviderConfiguration] = [
        .starterOllama(),
        .starterOpenAI()
    ]

    public init(persisted: ChatSettingsPersistedState? = nil) {
        if let persisted, !persisted.providers.isEmpty {
            providers = persisted.providers
            selectedProviderID = persisted.selectedProviderID ?? persisted.providers.first?.id
            selectedModel = persisted.selectedModel
        } else {
            providers = fallbackProviders
            selectedProviderID = fallbackProviders.first?.id
            selectedModel = fallbackProviders.first?.defaultModel ?? ""
        }

        if activeProvider == nil {
            selectedProviderID = providers.first?.id
        }
    }

    public var activeProvider: ChatProviderConfiguration? {
        guard let selectedProviderID else { return nil }
        return providers.first { $0.id == selectedProviderID }
    }

    public var activeModel: String {
        let trimmed = selectedModel.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        return activeProvider?.defaultModel.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    public var persistedState: ChatSettingsPersistedState {
        ChatSettingsPersistedState(
            providers: providers,
            selectedProviderID: selectedProviderID,
            selectedModel: selectedModel
        )
    }

    public func selectProvider(_ id: UUID?) {
        selectedProviderID = id
        if let provider = activeProvider {
            selectedModel = provider.defaultModel
        } else {
            selectedModel = ""
        }
        availableModels = []
        refreshErrorMessage = nil
        onChange?()
    }

    public func upsertProvider(_ provider: ChatProviderConfiguration) {
        if let index = providers.firstIndex(where: { $0.id == provider.id }) {
            providers[index] = provider
        } else {
            providers.append(provider)
        }

        if selectedProviderID == nil {
            selectedProviderID = provider.id
        }

        if provider.id == selectedProviderID, selectedModel.isEmpty {
            selectedModel = provider.defaultModel
        }

        onChange?()
    }

    @discardableResult
    public func addProviderTemplate() -> ChatProviderConfiguration {
        let provider = ChatProviderConfiguration(
            name: "New Provider",
            kind: .openAICompatible,
            baseURL: "https://",
            token: "",
            defaultModel: ""
        )
        upsertProvider(provider)
        selectProvider(provider.id)
        return provider
    }

    public func removeSelectedProvider() {
        guard let selectedProviderID else { return }
        providers.removeAll { $0.id == selectedProviderID }

        if providers.isEmpty {
            providers = fallbackProviders
            self.selectedProviderID = providers.first?.id
        } else {
            self.selectedProviderID = providers.first?.id
        }

        selectedModel = activeProvider?.defaultModel ?? ""
        availableModels = []
        refreshErrorMessage = nil
        onChange?()
    }

    public func setSelectedModel(_ model: String) {
        selectedModel = model

        if let id = selectedProviderID, let index = providers.firstIndex(where: { $0.id == id }) {
            providers[index].defaultModel = model
        }

        onChange?()
    }

    public func setAvailableModels(_ models: [String]) {
        availableModels = models.sorted()

        if !availableModels.isEmpty {
            if !availableModels.contains(activeModel) {
                setSelectedModel(availableModels[0])
            }
        }
    }

    public func setRefreshError(_ message: String?) {
        refreshErrorMessage = message
    }
}
