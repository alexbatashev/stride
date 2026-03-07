import CoreFriday
import Foundation
import Observation

// MARK: - Starter providers

extension ChatProviderConfiguration {
    fileprivate static func starterOllama() -> ChatProviderConfiguration {
        ChatProviderConfiguration(
            id: UUID().uuidString,
            name: "Local Ollama",
            kind: .ollama,
            baseUrl: "http://localhost:11434",
            token: "",
            defaultModel: ""
        )
    }

    fileprivate static func starterOpenAI() -> ChatProviderConfiguration {
        ChatProviderConfiguration(
            id: UUID().uuidString,
            name: "OpenAI",
            kind: .openAiCompatible,
            baseUrl: "https://api.openai.com",
            token: "",
            defaultModel: "gpt-4.1"
        )
    }
}

// MARK: - Persistence helpers

private struct PersistedProvider: Codable {
    var id: String
    var name: String
    var kind: String
    var baseUrl: String
    var token: String
    var defaultModel: String
}

private struct PersistedSettings: Codable {
    var providers: [PersistedProvider]
    var selectedProviderID: String?
    var selectedModel: String
}

extension ChatProviderConfiguration {
    fileprivate var asPersistedProvider: PersistedProvider {
        PersistedProvider(
            id: id, name: name, kind: kind.id, baseUrl: baseUrl, token: token,
            defaultModel: defaultModel)
    }
}

private func configurationFromPersisted(_ p: PersistedProvider) -> ChatProviderConfiguration? {
    let kind: ChatProviderKind
    switch p.kind {
    case "openAICompatible": kind = .openAiCompatible
    case "ollama": kind = .ollama
    case "anthropic": kind = .anthropic
    case "mock": kind = .mock
    default: return nil
    }
    return ChatProviderConfiguration(
        id: p.id, name: p.name, kind: kind, baseUrl: p.baseUrl, token: p.token,
        defaultModel: p.defaultModel)
}

// MARK: - ChatProviderSettingsStore

@MainActor
@Observable
public final class ChatProviderSettingsStore {
    public var providers: [ChatProviderConfiguration]
    public var selectedProviderID: String?
    public var selectedModel: String
    public var availableModels: [String] = []
    public var isRefreshingModels = false
    public var refreshErrorMessage: String?

    public var onChange: (() -> Void)?

    public init() {
        if let loaded = Self.loadFromDefaults(), !loaded.providers.isEmpty {
            providers = loaded.providers
            selectedProviderID = loaded.selectedProviderID ?? loaded.providers.first?.id
            selectedModel = loaded.selectedModel
        } else {
            let defaults = [
                ChatProviderConfiguration.starterOllama(),
                ChatProviderConfiguration.starterOpenAI(),
            ]
            providers = defaults
            selectedProviderID = defaults.first?.id
            selectedModel = defaults.first?.defaultModel ?? ""
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
        if !trimmed.isEmpty { return trimmed }
        return activeProvider?.defaultModel.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
    }

    public func selectProvider(_ id: String?) {
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
            id: UUID().uuidString,
            name: "New Provider",
            kind: .openAiCompatible,
            baseUrl: "https://",
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
            let defaults = [
                ChatProviderConfiguration.starterOllama(),
                ChatProviderConfiguration.starterOpenAI(),
            ]
            providers = defaults
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
            let old = providers[index]
            providers[index] = ChatProviderConfiguration(
                id: old.id, name: old.name, kind: old.kind,
                baseUrl: old.baseUrl, token: old.token, defaultModel: model
            )
        }

        onChange?()
    }

    public func setAvailableModels(_ models: [String]) {
        availableModels = models.sorted()

        if !availableModels.isEmpty, !availableModels.contains(activeModel) {
            setSelectedModel(availableModels[0])
        }
    }

    public func setRefreshError(_ message: String?) {
        refreshErrorMessage = message
    }

    // MARK: - Persistence

    func save() {
        let persisted = PersistedSettings(
            providers: providers.map { $0.asPersistedProvider },
            selectedProviderID: selectedProviderID,
            selectedModel: selectedModel
        )
        if let data = try? JSONEncoder().encode(persisted) {
            UserDefaults.standard.set(data, forKey: "ChatProviderSettings")
        }
    }

    private static func loadFromDefaults() -> (
        providers: [ChatProviderConfiguration], selectedProviderID: String?, selectedModel: String
    )? {
        guard let data = UserDefaults.standard.data(forKey: "ChatProviderSettings"),
            let persisted = try? JSONDecoder().decode(PersistedSettings.self, from: data)
        else { return nil }
        let providers = persisted.providers.compactMap { configurationFromPersisted($0) }
        guard !providers.isEmpty else { return nil }
        return (providers, persisted.selectedProviderID, persisted.selectedModel)
    }
}
