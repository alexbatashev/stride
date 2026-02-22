import Foundation
import Observation

enum ChatProviderKind: String, Codable, CaseIterable, Identifiable, Sendable {
    case openAICompatible
    case ollama
    case anthropic
    case mock

    var id: String { rawValue }

    var displayName: String {
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

struct ChatProviderConfiguration: Codable, Identifiable, Equatable, Sendable {
    var id: UUID
    var name: String
    var kind: ChatProviderKind
    var baseURL: String
    var token: String
    var defaultModel: String

    init(
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

    static func starterOpenAI() -> Self {
        .init(
            name: "OpenAI",
            kind: .openAICompatible,
            baseURL: "https://api.openai.com",
            token: "",
            defaultModel: "gpt-4.1"
        )
    }

    static func starterOllama() -> Self {
        .init(
            name: "Local Ollama",
            kind: .ollama,
            baseURL: "http://localhost:11434",
            token: "",
            defaultModel: ""
        )
    }
}

@MainActor
@Observable
final class ChatProviderSettingsStore {
    private struct PersistedState: Codable {
        var providers: [ChatProviderConfiguration]
        var selectedProviderID: UUID?
        var selectedModel: String
    }

    private let defaults: UserDefaults
    private let storageKey = "Friday.ChatProviderSettings.v1"

    var providers: [ChatProviderConfiguration]
    var selectedProviderID: UUID?
    var selectedModel: String
    var availableModels: [String] = []
    var isRefreshingModels = false
    var refreshErrorMessage: String?

    private let fallbackProviders: [ChatProviderConfiguration] = [
        .starterOllama(),
        .starterOpenAI()
    ]

    init(defaults: UserDefaults = .standard) {
        self.defaults = defaults

        if
            let data = defaults.data(forKey: storageKey),
            let decoded = try? JSONDecoder().decode(PersistedState.self, from: data),
            !decoded.providers.isEmpty
        {
            providers = decoded.providers
            selectedProviderID = decoded.selectedProviderID ?? decoded.providers.first?.id
            selectedModel = decoded.selectedModel
        } else {
            providers = fallbackProviders
            selectedProviderID = fallbackProviders.first?.id
            selectedModel = fallbackProviders.first?.defaultModel ?? ""
            persist()
        }

        if activeProvider == nil {
            selectedProviderID = providers.first?.id
        }
    }

    var activeProvider: ChatProviderConfiguration? {
        guard let selectedProviderID else { return nil }
        return providers.first { $0.id == selectedProviderID }
    }

    var activeModel: String {
        let trimmed = selectedModel.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }
        return activeProvider?.defaultModel.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    func selectProvider(_ id: UUID?) {
        selectedProviderID = id
        if let provider = activeProvider {
            selectedModel = provider.defaultModel
        } else {
            selectedModel = ""
        }
        availableModels = []
        refreshErrorMessage = nil
        persist()
    }

    func upsertProvider(_ provider: ChatProviderConfiguration) {
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

        persist()
    }

    @discardableResult
    func addProviderTemplate() -> ChatProviderConfiguration {
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

    func removeSelectedProvider() {
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
        persist()
    }

    func setSelectedModel(_ model: String) {
        selectedModel = model

        if let id = selectedProviderID, let index = providers.firstIndex(where: { $0.id == id }) {
            providers[index].defaultModel = model
        }

        persist()
    }

    func setAvailableModels(_ models: [String]) {
        availableModels = models.sorted()

        if !availableModels.isEmpty {
            if !availableModels.contains(activeModel) {
                setSelectedModel(availableModels[0])
            }
        }
    }

    func setRefreshError(_ message: String?) {
        refreshErrorMessage = message
    }

    func persist() {
        let state = PersistedState(
            providers: providers,
            selectedProviderID: selectedProviderID,
            selectedModel: selectedModel
        )

        guard let data = try? JSONEncoder().encode(state) else { return }
        defaults.set(data, forKey: storageKey)
    }
}
