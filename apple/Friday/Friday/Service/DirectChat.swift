import Foundation
import LLMKit

protocol OpenAILikeTransport: Sendable {
    func listModels(provider: ChatProviderConfiguration) async throws -> [ModelDesc]
    func streamCompletion(provider: ChatProviderConfiguration, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error>
}

struct LLMKitTransport: OpenAILikeTransport {
    func listModels(provider: ChatProviderConfiguration) async throws -> [ModelDesc] {
        try await api(for: provider).listModels(token: provider.token)
    }

    func streamCompletion(provider: ChatProviderConfiguration, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        api(for: provider).streamCompletion(token: provider.token, request: request)
    }

    private func api(for provider: ChatProviderConfiguration) -> API {
        switch provider.kind {
        case .openAICompatible:
            return OpenAI.api(baseURL: provider.baseURL)
        case .ollama:
            return Ollama.api(baseURL: provider.baseURL)
        case .anthropic:
            return Anthropic.api(baseURL: provider.baseURL)
        case .mock:
            return Mock.api()
        }
    }
}

struct DirectChat: Sendable {
    private let transport: any OpenAILikeTransport

    init(transport: any OpenAILikeTransport = LLMKitTransport()) {
        self.transport = transport
    }

    func listModelIDs(provider: ChatProviderConfiguration) async throws -> [String] {
        let models = try await transport.listModels(provider: provider)
        return models.map(\.id).sorted()
    }

    func streamReply(
        provider: ChatProviderConfiguration,
        model: String,
        turns: [ConversationTurn]
    ) -> AsyncThrowingStream<String, Error> {
        let messages = turns
            .sorted { $0.sequenceNumber < $1.sequenceNumber }
            .map { Message(role: mapRole($0.role), content: $0.text) }

        let request = CompletionRequest(model: model, messages: messages).stream()

        let upstream = transport.streamCompletion(provider: provider, request: request)

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    for try await chunk in upstream {
                        let token = chunk.choices
                            .compactMap { choice in
                                choice.delta?.content ?? choice.message?.content ?? choice.text
                            }
                            .joined()

                        guard !token.isEmpty else { continue }
                        continuation.yield(token)
                    }

                    continuation.finish()
                } catch {
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
