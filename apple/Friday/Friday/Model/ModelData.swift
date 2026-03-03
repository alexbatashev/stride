import CoreFriday
import SwiftUI
import Foundation
import Observation

@Observable
@MainActor
final class ModelData {
    var selectedNavigation: NavigationOptions?
    var selectedThread: ChatThread?
    var selectedNote: Note?

    var searchString: String = ""

    var threads: [ChatThread] = []
    var notes: [Note] = []
    var chatSettings: ChatProviderSettingsStore

    private let storage: CoreFridayStorage

    init() {
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

        threads = (try? storage.listThreads()) ?? []
        notes = snapshot.notes
        chatSettings = ChatProviderSettingsStore(persisted: snapshot.chatSettings)

        chatSettings.onChange = { [weak self] in
            self?.persistAll()
        }

        ensureInitialThread()
        ensureInitialNote()

        if ProcessInfo.processInfo.arguments.contains("-ui-testing-open-notes") {
            selectedNavigation = .notes
        } else {
            selectedNavigation = .chat
        }
    }

    var sortedThreads: [ChatThread] {
        threads.sorted { $0.updatedAt > $1.updatedAt }
    }

    var sortedNotes: [Note] {
        notes.sorted { $0.updatedAt > $1.updatedAt }
    }

    func ensureInitialThread() {
        guard threads.isEmpty else {
            if selectedThread == nil {
                selectedThread = sortedThreads.first
            }
            return
        }

        let thread = ChatThread(
            userId: nil,
            title: "Welcome",
            createdAt: .now,
            updatedAt: .now,
            previewText: "Send a message to start chatting.",
            isPinned: false
        )
        threads.append(thread)
        selectedThread = thread
        try? storage.upsertThread(thread)
    }

    func createThread() {
        let thread = ChatThread(
            userId: nil,
            title: "",
            createdAt: .now,
            updatedAt: .now,
            previewText: "",
            isPinned: false
        )
        threads.append(thread)
        selectedThread = thread
        try? storage.upsertThread(thread)
    }

    func deleteThreads(at offsets: IndexSet) {
        let sorted = sortedThreads
        for offset in offsets {
            guard sorted.indices.contains(offset) else { continue }
            let thread = sorted[offset]
            threads.removeAll { $0.id == thread.id }
            if selectedThread?.id == thread.id {
                selectedThread = nil
            }
            try? storage.deleteThread(id: thread.id)
        }

        if selectedThread == nil {
            selectedThread = sortedThreads.first
        }
    }

    func ensureInitialNote() {
        guard notes.isEmpty else {
            if selectedNote == nil {
                selectedNote = sortedNotes.first
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
        selectedNote = note
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
        selectedNote = note
        persistAll()
    }

    func deleteNotes(at offsets: IndexSet) {
        let sorted = sortedNotes
        for offset in offsets {
            guard sorted.indices.contains(offset) else { continue }
            let note = sorted[offset]
            notes.removeAll { $0.id == note.id }
            if selectedNote?.id == note.id {
                selectedNote = nil
            }
        }

        if selectedNote == nil {
            selectedNote = sortedNotes.first
        }

        persistAll()
    }

    func persistAll() {
        do {
            try storage.replaceSnapshot(
                conversations: [],
                notes: notes,
                chatSettings: chatSettings.persistedState
            )
        } catch {
            assertionFailure("Failed to persist snapshot: \(error)")
        }
    }

    func getMessages(for thread: ChatThread) async -> [ChatMessage] {
        await storage.makeChatStorage(threadId: thread.id).listMessages()
    }

    func refreshModels() async {
        guard let provider = chatSettings.activeProvider else { return }

        chatSettings.isRefreshingModels = true
        chatSettings.setRefreshError(nil)
        defer { chatSettings.isRefreshingModels = false }

        let transport = DirectChatTransport(provider: provider)
        let models = await transport.listModels()
        if models.isEmpty {
            chatSettings.setRefreshError("No models returned. Check your provider configuration.")
        } else {
            chatSettings.setAvailableModels(models.map(\.model))
        }
    }

    func addMessage(text: String, in thread: ChatThread) async -> AsyncThrowingStream<ChatMessage, Error> {
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

        let providerId = provider.id.uuidString
        let transport = DirectChatTransport(provider: provider)
        let chatStorage = storage.makeChatStorage(threadId: thread.id)
        let service = ChatService(transports: [transport], storage: chatStorage)
        await service.setModel(providerId: providerId, modelId: model)

        let userMessage = ChatMessage(
            threadId: thread.id,
            providerId: providerId,
            modelId: model,
            modelName: model,
            role: .user,
            content: text
        )

        return await service.addMessage(tools: [], next: userMessage)
    }

    private static func defaultDatabasePath() -> String {
        let baseURL = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)

        let directory = baseURL.appendingPathComponent("Friday", isDirectory: true)
        return directory.appendingPathComponent("db.sqlite", isDirectory: false).path
    }
}
