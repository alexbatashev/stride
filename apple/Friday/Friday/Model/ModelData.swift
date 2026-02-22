import Foundation
import Observation

@Observable
@MainActor
final class ModelData {
    var selectedNavigation: NavigationOptions?
    var selectedConversationID: UUID?
    var selectedNoteID: UUID?

    var chatSettings = ChatProviderSettingsStore()
    private let directChat: DirectChat

    init() {
        self.directChat = DirectChat()

        if ProcessInfo.processInfo.arguments.contains("-ui-testing-open-notes") {
            selectedNavigation = .notes
        } else {
            selectedNavigation = .chat
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
}
