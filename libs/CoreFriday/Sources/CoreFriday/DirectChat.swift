import Foundation
import LLMKit

protocol OpenAILikeTransport: Sendable {
    func listModels(provider: ChatProviderConfiguration) async throws -> [ModelDesc]
    func streamResponse(provider: ChatProviderConfiguration, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error>
}

struct LLMKitTransport: OpenAILikeTransport {
    func listModels(provider: ChatProviderConfiguration) async throws -> [ModelDesc] {
        try await api(for: provider).listModels(token: provider.token)
    }

    func streamResponse(provider: ChatProviderConfiguration, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        api(for: provider).streamResponse(token: provider.token, request: request)
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

public struct DirectChat: Sendable {
    private let transport: any OpenAILikeTransport

    public init() {
        self.transport = LLMKitTransport()
    }

    init(transport: any OpenAILikeTransport) {
        self.transport = transport
    }

    public func listModelIDs(provider: ChatProviderConfiguration) async throws -> [String] {
        let models = try await transport.listModels(provider: provider)
        return models.map(\.id).sorted()
    }

    public func streamReply(
        provider: ChatProviderConfiguration,
        model: String,
        turns: [ConversationTurn]
    ) -> AsyncThrowingStream<String, Error> {
        let messages = turns
            .sorted { $0.createdAt < $1.createdAt }
            .map { Message(role: mapRole($0.role), content: $0.text) }

        let request = ResponseRequest(model: model, input: messages).stream()

        let upstream = transport.streamResponse(provider: provider, request: request)

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    for try await chunk in upstream {
                        let token = chunk.delta ?? chunk.text ?? ""

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
