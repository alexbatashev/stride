import CoreFriday
import Foundation
import Observation
import SwiftUI

@Observable
@MainActor
public final class ModelData {
    // Tool execution policy is app-layer only; keep it out of SwiftUI views.
    private let toolExecutionPolicy = ToolExecutionPolicy()

    var selectedNavigation: NavigationOptions?
    var selectedThread: ChatThread?

    var searchString: String = ""

    var threads: [ChatThread] = []
    var chatSettings: ChatProviderSettingsStore

    private let database: ChatDatabase

    public init() {
        let storagePath = Self.defaultDatabasePath()
        try? FileManager.default.createDirectory(
            at: URL(fileURLWithPath: storagePath).deletingLastPathComponent(),
            withIntermediateDirectories: true
        )

        database = ChatDatabase.open(path: storagePath)
        chatSettings = ChatProviderSettingsStore()
        chatSettings.onChange = { [weak self] in
            self?.chatSettings.save()
        }

        selectedNavigation = .chat
    }

    var sortedThreads: [ChatThread] {
        threads.sorted { $0.updatedAtMs > $1.updatedAtMs }
    }

    func loadThreads() async {
        threads = await database.listThreads()
        ensureInitialThread()
    }

    func ensureInitialThread() {
        guard threads.isEmpty else {
            if selectedThread == nil {
                selectedThread = sortedThreads.first
            }
            return
        }

        let now = Int64(Date().timeIntervalSince1970 * 1000)
        let thread = ChatThread(
            id: UUID().uuidString,
            userId: nil,
            title: "Welcome",
            createdAtMs: now,
            updatedAtMs: now,
            previewText: "Send a message to start chatting.",
            isPinned: false
        )
        threads.append(thread)
        selectedThread = thread
        Task { await database.upsertThread(thread: thread) }
    }

    func createThread() {
        let now = Int64(Date().timeIntervalSince1970 * 1000)
        let thread = ChatThread(
            id: UUID().uuidString,
            userId: nil,
            title: "",
            createdAtMs: now,
            updatedAtMs: now,
            previewText: "",
            isPinned: false
        )
        threads.append(thread)
        selectedThread = thread
        Task { await database.upsertThread(thread: thread) }
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
            let threadId = thread.id
            Task { await database.deleteThread(threadId: threadId) }
        }

        if selectedThread == nil {
            selectedThread = sortedThreads.first
        }
    }

    func updateThread(_ threadId: String, previewText: String) {
        guard let idx = threads.firstIndex(where: { $0.id == threadId }) else { return }
        let updated = threads[idx].withPreviewText(previewText)
        threads[idx] = updated
        Task { await database.upsertThread(thread: updated) }
    }

    func getMessages(for thread: ChatThread) async -> [ChatMessage] {
        await database.listMessages(threadId: thread.id)
    }

    func fetchModels(for provider: ChatProviderConfiguration) async throws -> [LangModel] {
        let service = ChatService.newWithProviders(providers: [provider])
        let models = await service.listModels()
        guard !models.isEmpty else {
            throw NSError(
                domain: "Friday.Models", code: 0,
                userInfo: [
                    NSLocalizedDescriptionKey:
                        "No models returned. Check your provider configuration."
                ])
        }
        return models
    }

    func refreshModels() async {
        guard let provider = chatSettings.activeProvider else { return }

        chatSettings.isRefreshingModels = true
        chatSettings.setRefreshError(nil)
        defer { chatSettings.isRefreshingModels = false }

        let service = ChatService.newWithProviders(providers: [provider])
        let models = await service.listModels()
        if models.isEmpty {
            chatSettings.setRefreshError("No models returned. Check your provider configuration.")
        } else {
            chatSettings.setAvailableModels(models.map { $0.model })
        }
    }

    func addMessage(text: String, in thread: ChatThread) async -> AsyncThrowingStream<
        ChatMessage, Error
    > {
        guard let provider = chatSettings.activeProvider else {
            return AsyncThrowingStream { continuation in
                continuation.finish(
                    throwing: NSError(
                        domain: "Friday.Chat", code: 1,
                        userInfo: [NSLocalizedDescriptionKey: "No provider selected."]))
            }
        }

        let model = chatSettings.activeModel
        guard !model.isEmpty else {
            return AsyncThrowingStream { continuation in
                continuation.finish(
                    throwing: NSError(
                        domain: "Friday.Chat", code: 2,
                        userInfo: [NSLocalizedDescriptionKey: "Select a model before sending."]))
            }
        }

        let service = await database.makeService(threadId: thread.id, providers: [provider])
        await service.setModel(providerId: provider.id, modelId: model)

        let userMessage = ChatMessage.makeNew(threadId: thread.id, role: .user, content: text)
        return service.addMessage(
            toolsEnabled: toolExecutionPolicy.areToolsEnabled,
            next: userMessage
        )
    }

    private static func defaultDatabasePath() -> String {
        let baseURL =
            FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory(), isDirectory: true)

        let directory = baseURL.appendingPathComponent("Friday", isDirectory: true)
        return directory.appendingPathComponent("db.sqlite", isDirectory: false).path
    }
}

private struct ToolExecutionPolicy {
    // Enable tools unconditionally for now.
    let areToolsEnabled = true
}
