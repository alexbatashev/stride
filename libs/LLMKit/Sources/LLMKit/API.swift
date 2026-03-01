import Foundation

public enum API: Sendable {
    case openAI(OpenAI)
    case anthropic(Anthropic)
    case ollama(Ollama)
    case mock(Mock)

    public func listModels(token: String) async throws -> [ModelDesc] {
        switch self {
        case .openAI(let api):
            return try await api.listModels(token: token)
        case .anthropic(let api):
            return try await api.listModels(token: token)
        case .ollama(let api):
            return try await api.listModels(token: token)
        case .mock(let api):
            return try await api.listModels(token: token)
        }
    }

    public func getModel(token: String, modelName: String) async throws -> ModelDesc {
        switch self {
        case .openAI(let api):
            return try await api.getModel(token: token, model: modelName)
        case .anthropic(let api):
            return try await api.getModel(token: token, model: modelName)
        case .ollama(let api):
            return try await api.getModel(token: token, model: modelName)
        case .mock(let api):
            return try await api.getModel(token: token, model: modelName)
        }
    }

    public func getEmbeddings(token: String, input: String, model: String) async throws -> EmbeddingResponse {
        switch self {
        case .openAI(let api):
            return try await api.getEmbeddings(token: token, input: input, model: model)
        case .ollama(let api):
            return try await api.getEmbeddings(token: token, input: input, model: model)
        case .anthropic, .mock:
            throw LLMError.invalidRequest("embeddings are not implemented for this provider")
        }
    }

    public func getCompletion(token: String, request: CompletionRequest) async throws -> Completion {
        if request.isStream == true {
            throw LLMError.invalidRequest("expected stream == false")
        }

        switch self {
        case .openAI(let api):
            return try await api.getCompletion(token: token, request: request)
        case .anthropic(let api):
            return try await api.getCompletion(token: token, request: request)
        case .ollama(let api):
            return try await api.getCompletion(token: token, request: request)
        case .mock(let api):
            return try await api.getCompletion(token: token, request: request)
        }
    }

    public func streamCompletion(token: String, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        let streamingRequest = request.stream()

        switch self {
        case .openAI(let api):
            return api.streamCompletion(token: token, request: streamingRequest)
        case .anthropic(let api):
            return api.streamCompletion(token: token, request: streamingRequest)
        case .ollama(let api):
            return api.streamCompletion(token: token, request: streamingRequest)
        case .mock(let api):
            return api.streamCompletion(token: token, request: streamingRequest)
        }
    }


    public func getResponse(token: String, request: ResponseRequest) async throws -> Response {
        if request.isStream == true {
            throw LLMError.invalidRequest("expected stream == false")
        }

        switch self {
        case .openAI(let api):
            return try await api.getResponse(token: token, request: request)
        case .anthropic(let api):
            return try await api.getResponse(token: token, request: request)
        case .ollama(let api):
            return try await api.getResponse(token: token, request: request)
        case .mock(let api):
            return try await api.getResponse(token: token, request: request)
        }
    }

    public func streamResponse(token: String, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        let streamingRequest = request.stream()

        switch self {
        case .openAI(let api):
            return api.streamResponse(token: token, request: streamingRequest)
        case .anthropic(let api):
            return api.streamResponse(token: token, request: streamingRequest)
        case .ollama(let api):
            return api.streamResponse(token: token, request: streamingRequest)
        case .mock(let api):
            return api.streamResponse(token: token, request: streamingRequest)
        }
    }

}
