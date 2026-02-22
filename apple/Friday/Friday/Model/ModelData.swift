import CoreFriday
import Foundation
import Observation

@Observable
@MainActor
final class ModelData {
    var selectedNavigation: NavigationOptions?
    var selectedConversationID: UUID?
    var selectedNoteID: UUID?

    var conversations: [Conversation] = []
    var notes: [Note] = []
    var chatSettings: ChatProviderSettingsStore

    private let directChat: DirectChat
    private let storage: CoreFridayStorage

    init() {
        self.directChat = DirectChat()

        let storagePath = Self.defaultDatabasePath()
        try? FileManager.default.createDirectory(
            at: URL(fileURLWithPath: storagePath).deletingLastPathComponent(),
            withIntermediateDirectories: true
        )

        do {
            storage = try CoreFridayStorage(databaseFilePath: storagePath)
        } catch {
            fatalError("Could not initialize CoreFridayStorage: \(error)")
        }

        let snapshot = (try? storage.loadSnapshot()) ?? CoreFridaySnapshot(
            conversations: [],
            notes: [],
            chatSettings: nil
        )

        conversations = snapshot.conversations
        notes = snapshot.notes
        chatSettings = ChatProviderSettingsStore(persisted: snapshot.chatSettings)

        chatSettings.onChange = { [weak self] in
            self?.persistAll()
        }

        ensureInitialConversation()
        ensureInitialNote()

        if ProcessInfo.processInfo.arguments.contains("-ui-testing-open-notes") {
            selectedNavigation = .notes
        } else {
            selectedNavigation = .chat
        }
    }

    var sortedConversations: [Conversation] {
        conversations.sorted { $0.updatedAt > $1.updatedAt }
    }

    var sortedNotes: [Note] {
        notes.sorted { $0.updatedAt > $1.updatedAt }
    }

    var selectedConversation: Conversation? {
        if let selectedConversationID {
            return conversations.first { $0.id == selectedConversationID }
        }
        return sortedConversations.first
    }

    var selectedNote: Note? {
        if let selectedNoteID {
            return notes.first { $0.id == selectedNoteID }
        }
        return sortedNotes.first
    }

    func ensureInitialConversation() {
        guard conversations.isEmpty else {
            if selectedConversationID == nil {
                selectedConversationID = sortedConversations.first?.id
            }
            return
        }

        let conversation = Conversation(title: "Welcome")

        let turn = ConversationTurn(
            role: .assistant,
            text: "Welcome to Friday. Send a message to start a local, Fluent + SQLite-backed chat.",
            sequenceNumber: 0,
            modelIdentifier: "local.stub.v1"
        )

        conversation.turns.append(turn)
        conversation.refreshPreview(using: turn.text)

        conversations.append(conversation)
        selectedConversationID = conversation.id
        persistAll()
    }

    func createConversation() {
        let conversation = Conversation()
        conversations.append(conversation)
        selectedConversationID = conversation.id
        persistAll()
    }

    func deleteConversations(at offsets: IndexSet) {
        let sorted = sortedConversations
        for offset in offsets {
            guard sorted.indices.contains(offset) else { continue }
            let id = sorted[offset].id
            conversations.removeAll { $0.id == id }
            if selectedConversationID == id {
                selectedConversationID = nil
            }
        }

        if selectedConversationID == nil {
            selectedConversationID = sortedConversations.first?.id
        }

        persistAll()
    }

    func ensureInitialNote() {
        guard notes.isEmpty else {
            if selectedNoteID == nil {
                selectedNoteID = sortedNotes.first?.id
            }
            return
        }

        let note = Note(title: "Welcome")
        let block = NoteBlock(
            kind: .text,
            orderIndex: 0,
            textContent: "Welcome to Notes. This prototype stores flexible block-based note data with Fluent + SQLite."
        )

        note.blocks.append(block)
        note.refreshPreview()

        notes.append(note)
        selectedNoteID = note.id
        persistAll()
    }

    func createNote() {
        let note = Note()
        let block = NoteBlock(
            kind: .text,
            orderIndex: note.nextOrderIndex,
            textContent: ""
        )

        note.blocks.append(block)
        note.refreshPreview()

        notes.append(note)
        selectedNoteID = note.id
        persistAll()
    }

    func deleteNotes(at offsets: IndexSet) {
        let sorted = sortedNotes
        for offset in offsets {
            guard sorted.indices.contains(offset) else { continue }
            let id = sorted[offset].id
            notes.removeAll { $0.id == id }
            if selectedNoteID == id {
                selectedNoteID = nil
            }
        }

        if selectedNoteID == nil {
            selectedNoteID = sortedNotes.first?.id
        }

        persistAll()
    }

    func persistAll() {
        do {
            try storage.replaceSnapshot(
                conversations: conversations,
                notes: notes,
                chatSettings: chatSettings.persistedState
            )
        } catch {
            assertionFailure("Failed to persist snapshot: \(error)")
        }
    }

    func refreshModels() async {
        guard let provider = chatSettings.activeProvider else { return }

        chatSettings.isRefreshingModels = true
        chatSettings.setRefreshError(nil)
        defer { chatSettings.isRefreshingModels = false }

        do {
            let modelIDs = try await directChat.listModelIDs(provider: provider)
            chatSettings.setAvailableModels(modelIDs)
        } catch {
            chatSettings.setRefreshError(error.localizedDescription)
        }
    }

    func streamAssistantReply(turns: [ConversationTurn]) -> AsyncThrowingStream<String, Error> {
        guard let provider = chatSettings.activeProvider else {
            return AsyncThrowingStream { continuation in
                continuation.finish(throwing: NSError(domain: "Friday.Chat", code: 1, userInfo: [NSLocalizedDescriptionKey: "No provider selected."]))
            }
        }

        let model = chatSettings.activeModel
        guard !model.isEmpty else {
            return AsyncThrowingStream { continuation in
                continuation.finish(throwing: NSError(domain: "Friday.Chat", code: 2, userInfo: [NSLocalizedDescriptionKey: "Select a model before sending."]))
            }
        }

        return directChat.streamReply(provider: provider, model: model, turns: turns)
    }

    private static func defaultDatabasePath() -> String {
        let baseURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)

        let directory = baseURL.appendingPathComponent("Friday", isDirectory: true)
        return directory.appendingPathComponent("db.sqlite", isDirectory: false).path
    }
}
