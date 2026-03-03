import Foundation

public struct Ollama: Sendable {
    public let baseURL: String

    public init(baseURL: String) {
        self.baseURL = baseURL
    }

    public static func api(baseURL: String) -> API {
        .ollama(Ollama(baseURL: baseURL))
    }

    public func listModels(token _: String) async throws -> [ModelDesc] {
        struct ModelEntry: Decodable {
            let model: String
        }

        struct Models: Decodable {
            let models: [ModelEntry]
        }

        let (response, data) = try await HTTPClient.request(
            method: "GET",
            url: try endpoint("/api/tags")
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            let list = try JSONDecoder().decode(Models.self, from: data)
            return list.models.map {
                ModelDesc(id: $0.model, object: "model", created: nil, ownedBy: nil)
            }
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getModel(token _: String, model: String) async throws -> ModelDesc {
        struct Body: Encodable {
            let model: String
        }

        let body = try JSONEncoder().encode(Body(model: model))

        let (response, _) = try await HTTPClient.request(
            method: "POST",
            url: try endpoint("/api/show"),
            headers: ["Content-Type": "application/json"],
            body: body
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        return ModelDesc(id: model, object: "model", created: nil, ownedBy: nil)
    }

    public func getEmbeddings(token _: String, input: String, model: String) async throws -> EmbeddingResponse {
        struct RequestData: Encodable {
            let input: String
            let model: String
        }

        struct OllamaResponse: Decodable {
            let model: String
            let embeddings: [[Float]]
            let promptEvalCount: UInt32

            enum CodingKeys: String, CodingKey {
                case model
                case embeddings
                case promptEvalCount = "prompt_eval_count"
            }
        }

        let body = try JSONEncoder().encode(RequestData(input: input, model: model))

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: try endpoint("/api/embed"),
            headers: ["Content-Type": "application/json"],
            body: body
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            let upstream = try JSONDecoder().decode(OllamaResponse.self, from: data)
            guard let firstEmbedding = upstream.embeddings.first else {
                throw LLMError.parsingError("missing embeddings")
            }
            return EmbeddingResponse(
                object: "object",
                model: upstream.model,
                data: EmbeddingData(object: "list", index: 0, embedding: firstEmbedding),
                usage: Usage(
                    promptTokens: upstream.promptEvalCount,
                    completionTokens: 0,
                    totalTokens: upstream.promptEvalCount
                )
            )
        } catch {
            if let llmError = error as? LLMError {
                throw llmError
            }
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getCompletion(token _: String, request: CompletionRequest) async throws -> Completion {
        struct ChatRequest: Encodable {
            let model: String
            let stream: Bool
            let messages: [Message]
        }

        struct MessageResponse: Decodable {
            let model: String
            let message: Message
            let done: Bool
            let doneReason: String?
            let promptEvalCount: UInt32?
            let evalCount: UInt32?

            enum CodingKeys: String, CodingKey {
                case model
                case message
                case done
                case doneReason = "done_reason"
                case promptEvalCount = "prompt_eval_count"
                case evalCount = "eval_count"
            }
        }

        let body = try JSONEncoder().encode(
            ChatRequest(model: request.model, stream: false, messages: request.messages)
        )

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: try endpoint("/api/chat"),
            headers: ["Content-Type": "application/json"],
            body: body
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            let message = try JSONDecoder().decode(MessageResponse.self, from: data)
            let promptTokens = message.promptEvalCount ?? 0
            let completionTokens = message.evalCount ?? 0

            return Completion(
                id: UUID().uuidString.lowercased(),
                created: 0,
                model: message.model,
                choices: [
                    CompletionChoice(
                        message: message.message,
                        text: nil,
                        index: 0,
                        delta: nil,
                        logprobs: nil,
                        finishReason: message.doneReason
                    )
                ],
                usage: Usage(
                    promptTokens: promptTokens,
                    completionTokens: completionTokens,
                    totalTokens: promptTokens + completionTokens
                )
            )
        } catch {
            throw LLMError.parsingError("Failed to parse upstream response: \(error)")
        }
    }

    public func streamCompletion(token _: String, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        struct ChatRequest: Encodable {
            let model: String
            let stream: Bool
            let messages: [Message]
        }

        struct MessageResponse: Decodable {
            let model: String
            let message: Message
            let done: Bool
            let doneReason: String?

            enum CodingKeys: String, CodingKey {
                case model
                case message
                case done
                case doneReason = "done_reason"
            }
        }

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    let body = try JSONEncoder().encode(
                        ChatRequest(model: request.model, stream: true, messages: request.messages)
                    )

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: try endpoint("/api/chat"),
                        headers: ["Content-Type": "application/json"],
                        body: body
                    )

                    for try await line in lineStream {
                        guard !line.isEmpty else { continue }
                        guard let json = line.data(using: .utf8) else { continue }

                        let data: MessageResponse
                        do {
                            data = try JSONDecoder().decode(MessageResponse.self, from: json)
                        } catch {
                            throw LLMError.parsingError(String(describing: error))
                        }

                        let chunk = StreamResponseChunk(
                            id: UUID().uuidString.lowercased(),
                            object: "completion",
                            created: 0,
                            model: data.model,
                            systemFingerprint: nil,
                            choices: [
                                CompletionChoice(
                                    message: data.message,
                                    text: nil,
                                    index: 0,
                                    delta: Delta(content: data.message.content),
                                    logprobs: nil,
                                    finishReason: data.doneReason
                                )
                            ]
                        )
                        continuation.yield(chunk)

                        if data.done {
                            continuation.finish()
                            return
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    public func getResponse(token _: String, request: ResponseRequest) async throws -> Response {
        struct OllamaToolFunction: Encodable {
            let name: String
            let description: String
            let parameters: FunctionParameters?
        }

        struct OllamaTool: Encodable {
            let type: String
            let function: OllamaToolFunction
        }

        struct ChatRequest: Encodable {
            let model: String
            let stream: Bool
            let messages: [Message]
            let tools: [OllamaTool]?
            let think: Bool?
        }

        struct ToolFunctionCall: Decodable {
            let function: ToolFunction
        }

        struct ToolFunction: Decodable {
            let name: String
            let arguments: [String: JSONValue]
        }

        struct MessageWithToolCalls: Decodable {
            let role: Role
            let content: String
            let thinking: String?
            let toolCalls: [ToolFunctionCall]?

            enum CodingKeys: String, CodingKey {
                case role
                case content
                case thinking
                case toolCalls = "tool_calls"
            }
        }

        struct MessageResponse: Decodable {
            let model: String
            let message: MessageWithToolCalls
            let promptEvalCount: UInt32?
            let evalCount: UInt32?

            enum CodingKeys: String, CodingKey {
                case model
                case message
                case promptEvalCount = "prompt_eval_count"
                case evalCount = "eval_count"
            }
        }

        let body = try JSONEncoder().encode(
            ChatRequest(
                model: request.model,
                stream: false,
                messages: request.input,
                tools: request.tools?.map {
                    OllamaTool(
                        type: $0.type.rawValue,
                        function: OllamaToolFunction(
                            name: $0.function.name,
                            description: $0.function.description,
                            parameters: $0.function.parameters?.first
                        )
                    )
                },
                think: true
            )
        )

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: try endpoint("/api/chat"),
            headers: ["Content-Type": "application/json"],
            body: body
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            let message = try JSONDecoder().decode(MessageResponse.self, from: data)
            let promptTokens = message.promptEvalCount ?? 0
            let completionTokens = message.evalCount ?? 0
            var output: [ResponseOutput] = []

            if !message.message.content.isEmpty {
                output.append(
                    ResponseOutput(
                        type: "message",
                        role: message.message.role,
                        content: [ResponseContent(type: "output_text", text: message.message.content)]
                    )
                )
            }
            if let thinking = message.message.thinking, !thinking.isEmpty {
                output.append(
                    ResponseOutput(
                        type: "reasoning",
                        role: message.message.role,
                        content: [ResponseContent(type: "reasoning", text: thinking)]
                    )
                )
            }

            if let toolCalls = message.message.toolCalls {
                for call in toolCalls {
                    output.append(
                        ResponseOutput(
                            type: "function_call",
                            name: call.function.name,
                            arguments: stringifyJSON(call.function.arguments) ?? "{}"
                        )
                    )
                }
            }

            return Response(
                id: UUID().uuidString.lowercased(),
                model: message.model,
                output: output,
                usage: Usage(
                    promptTokens: promptTokens,
                    completionTokens: completionTokens,
                    totalTokens: promptTokens + completionTokens
                )
            )
        } catch {
            throw LLMError.parsingError("Failed to parse upstream response: \(error)")
        }
    }

    public func streamResponse(token _: String, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        struct OllamaToolFunction: Encodable {
            let name: String
            let description: String
            let parameters: FunctionParameters?
        }

        struct OllamaTool: Encodable {
            let type: String
            let function: OllamaToolFunction
        }

        struct ChatRequest: Encodable {
            let model: String
            let stream: Bool
            let messages: [Message]
            let tools: [OllamaTool]?
            let think: Bool?
        }

        struct MessageResponse: Decodable {
            let done: Bool
            let message: MessageWithToolCalls
        }

        struct ToolFunctionCall: Decodable {
            let function: ToolFunction
        }

        struct ToolFunction: Decodable {
            let name: String
            let arguments: [String: JSONValue]
        }

        struct MessageWithToolCalls: Decodable {
            let role: Role
            let content: String
            let thinking: String?
            let toolCalls: [ToolFunctionCall]?

            enum CodingKeys: String, CodingKey {
                case role
                case content
                case thinking
                case toolCalls = "tool_calls"
            }
        }

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    let body = try JSONEncoder().encode(
                        ChatRequest(
                            model: request.model,
                            stream: true,
                            messages: request.input,
                            tools: request.tools?.map {
                                OllamaTool(
                                    type: $0.type.rawValue,
                                    function: OllamaToolFunction(
                                        name: $0.function.name,
                                        description: $0.function.description,
                                        parameters: $0.function.parameters?.first
                                    )
                                )
                            },
                            think: true
                        )
                    )

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: try endpoint("/api/chat"),
                        headers: ["Content-Type": "application/json"],
                        body: body
                    )

                    for try await line in lineStream {
                        guard !line.isEmpty else { continue }
                        guard let json = line.data(using: .utf8) else { continue }

                        let data = try JSONDecoder().decode(MessageResponse.self, from: json)
                        if let thinking = data.message.thinking, !thinking.isEmpty {
                            continuation.yield(
                                ResponseStreamEvent(
                                    type: "response.reasoning.delta",
                                    responseID: nil,
                                    outputIndex: 0,
                                    delta: thinking,
                                    text: nil
                                )
                            )
                        }

                        if !data.message.content.isEmpty {
                            continuation.yield(
                                ResponseStreamEvent(
                                    type: "response.output_text.delta",
                                    responseID: nil,
                                    outputIndex: 0,
                                    delta: data.message.content,
                                    text: nil
                                )
                            )
                        }

                        if let toolCalls = data.message.toolCalls {
                            for call in toolCalls {
                                continuation.yield(
                                    ResponseStreamEvent(
                                        type: "response.function_call",
                                        responseID: nil,
                                        outputIndex: 0,
                                        delta: nil,
                                        text: nil,
                                        name: call.function.name,
                                        arguments: stringifyJSON(call.function.arguments) ?? "{}",
                                        callID: nil
                                    )
                                )
                            }
                        }

                        if data.done {
                            continuation.yield(
                                ResponseStreamEvent(
                                    type: "response.completed",
                                    responseID: nil,
                                    outputIndex: nil,
                                    delta: nil,
                                    text: nil
                                )
                            )
                            continuation.finish()
                            return
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private func endpoint(_ path: String) throws -> URL {
        guard let url = URL(string: "\(baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")))\(path)") else {
            throw LLMError.invalidRequest("invalid URL")
        }
        return url
    }
}

private enum JSONValue: Codable {
    case string(String)
    case number(Double)
    case bool(Bool)
    case object([String: JSONValue])
    case array([JSONValue])
    case null

    init(from decoder: Decoder) throws {
        let container = try decoder.singleValueContainer()
        if container.decodeNil() {
            self = .null
        } else if let value = try? container.decode(Bool.self) {
            self = .bool(value)
        } else if let value = try? container.decode(Double.self) {
            self = .number(value)
        } else if let value = try? container.decode(String.self) {
            self = .string(value)
        } else if let value = try? container.decode([String: JSONValue].self) {
            self = .object(value)
        } else if let value = try? container.decode([JSONValue].self) {
            self = .array(value)
        } else {
            throw DecodingError.typeMismatch(
                JSONValue.self,
                DecodingError.Context(codingPath: decoder.codingPath, debugDescription: "Invalid JSON")
            )
        }
    }

    func encode(to encoder: Encoder) throws {
        var container = encoder.singleValueContainer()
        switch self {
        case .string(let value):
            try container.encode(value)
        case .number(let value):
            try container.encode(value)
        case .bool(let value):
            try container.encode(value)
        case .object(let value):
            try container.encode(value)
        case .array(let value):
            try container.encode(value)
        case .null:
            try container.encodeNil()
        }
    }
}

private func stringifyJSON<T: Encodable>(_ value: T) -> String? {
    guard let data = try? JSONEncoder().encode(value) else { return nil }
    return String(data: data, encoding: .utf8)
}
