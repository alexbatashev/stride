import Foundation
import LLMKit

public protocol ChatTransport: Sendable {
    var providerId: String { get }
    func listModels() async -> [LangModel]
    func streamResponse(
        modelId: String,
        messages: [ConversationTurn]
    ) -> AsyncThrowingStream<ConversationTurn, Error>
}

public protocol ChatStorage: Sendable {}

public struct DirectChatTransport: ChatTransport {
    public let providerId: String
    private let api: API
    private let token: String

    public init(providerId: String, api: API, token: String = "") {
        self.providerId = providerId
        self.api = api
        self.token = token
    }

    public init(provider: ChatProviderConfiguration) {
        self.providerId = provider.id.uuidString
        self.token = provider.token
        self.api =
            switch provider.kind {
            case .openAICompatible:
                OpenAI.api(baseURL: provider.baseURL)
            case .ollama:
                Ollama.api(baseURL: provider.baseURL)
            case .anthropic:
                Anthropic.api(baseURL: provider.baseURL)
            case .mock:
                Mock.api()
            }
    }

    public func listModels() async -> [LangModel] {
        do {
            return try await api.listModels(token: token)
                .map { model in
                    LangModel(
                        provider: providerId,
                        model: model.id,
                        providerName: providerId,
                        modelName: model.id
                    )
                }
                .sorted { $0.model < $1.model }
        } catch {
            return []
        }
    }

    public func streamResponse(
        modelId: String,
        messages: [ConversationTurn]
    ) -> AsyncThrowingStream<ConversationTurn, Error> {
        let sortedMessages = messages.sorted { $0.createdAt < $1.createdAt }

        let request = ResponseRequest(
            model: modelId,
            input: sortedMessages.map { Message(role: mapRole($0.role), content: $0.text) }
        ).stream()

        let upstream = api.streamResponse(token: token, request: request)

        return AsyncThrowingStream { continuation in
            Task {
                let responseTurn = ConversationTurn(
                    role: .assistant,
                    text: "",
                    modelIdentifier: modelId
                )

                do {
                    for try await event in upstream {
                        let token = event.delta ?? event.text ?? ""
                        guard !token.isEmpty else { continue }

                        responseTurn.text += token
                        continuation.yield(responseTurn)
                    }
                    continuation.finish()
                } catch {
                    responseTurn.isError = true
                    continuation.yield(responseTurn)
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private func mapRole(_ role: TurnRole) -> Role {
        switch role {
        case .system:
            return .system
        case .user:
            return .user
        case .assistant:
            return .assistant
        case .tool:
            return .tool
        }
    }
}

public struct NullStorage: ChatStorage {
    public init() {}
}

public enum ChatStreamError: Error {
    case providerNotSelected
    case modelNotSelected
    case unknownProvider(String)
}

public actor ChatStream {
    private let transports: [any ChatTransport]
    private let storage: any ChatStorage

    private var providerId: String?
    private var modelId: String?

    private var messages: [ConversationTurn]

    public init(
        transports: [any ChatTransport],
        storage: ChatStorage = NullStorage(),
        messages: [ConversationTurn] = []
    ) {
        self.transports = transports.sorted { $0.providerId < $1.providerId }
        self.storage = storage
        self.messages = messages.sorted { $0.createdAt < $1.createdAt }
    }

    public func listModels() async -> [LangModel] {
        var merged: [LangModel] = []

        for transport in transports {
            for model in await transport.listModels() {
                merged.append(model)
            }
        }

        return merged
    }

    public func setModel(providerId: String, modelId: String) {
        // TODO check provider with the requested providerId exists
        self.providerId = providerId
        // Model ID is stored as-is as the provider will return an error if the model does not exist during request
        self.modelId = modelId
    }

    public func addMessage(tools: [Tool], next: ConversationTurn) -> AsyncThrowingStream<
        ConversationTurn, Error
    > {
        messages.append(next)

        guard let providerId else {
            return Self.failedStream(ChatStreamError.providerNotSelected)
        }
        guard let modelId else {
            return Self.failedStream(ChatStreamError.modelNotSelected)
        }
        guard let transport = transports.first(where: { $0.providerId == providerId }) else {
            return Self.failedStream(ChatStreamError.unknownProvider(providerId))
        }

        let upstream = transport.streamResponse(modelId: modelId, messages: messages)

        return AsyncThrowingStream { continuation in
            Task {
                var latest: ConversationTurn?

                do {
                    for try await partial in upstream {
                        latest = partial
                        continuation.yield(partial)
                    }

                    if let latest {
                        self.appendMessage(latest)
                    }
                    continuation.finish()
                } catch {
                    if let latest {
                        self.appendMessage(latest)
                    }
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private static func failedStream(_ error: Error) -> AsyncThrowingStream<ConversationTurn, Error>
    {
        AsyncThrowingStream { continuation in
            continuation.finish(throwing: error)
        }
    }

    private func appendMessage(_ message: ConversationTurn) {
        messages.append(message)
    }
}
