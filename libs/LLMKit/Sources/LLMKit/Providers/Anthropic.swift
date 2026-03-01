import Foundation

public struct Anthropic: Sendable {
    public let baseURL: String

    public init(baseURL: String) {
        self.baseURL = baseURL
    }

    public static func api(baseURL: String) -> API {
        .anthropic(Anthropic(baseURL: baseURL))
    }

    public func listModels(token: String) async throws -> [ModelDesc] {
        struct AnthropicModelList: Decodable {
            let models: [ModelDesc]
        }

        let (response, data) = try await HTTPClient.request(
            method: "GET",
            url: try endpoint("/v1/models"),
            headers: baseHeaders(token: token)
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try JSONDecoder().decode(AnthropicModelList.self, from: data).models
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getModel(token: String, model: String) async throws -> ModelDesc {
        let (response, data) = try await HTTPClient.request(
            method: "GET",
            url: try endpoint("/v1/models/\(model)"),
            headers: baseHeaders(token: token)
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try JSONDecoder().decode(ModelDesc.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getCompletion(token: String, request: CompletionRequest) async throws -> Completion {
        let upstream = try await postMessages(
            token: token,
            body: try messagesRequestBody(
                model: request.model,
                maxTokens: request.maxTokens ?? 8192,
                messages: request.messages,
                stream: nil
            )
        )

        let choices = upstream.content.enumerated().map { index, content in
            CompletionChoice(
                message: nil,
                text: content.text,
                index: UInt16(index),
                delta: nil,
                logprobs: nil,
                finishReason: upstream.stopReason
            )
        }

        let prompt = upstream.usage.inputTokens
        let completion = upstream.usage.outputTokens
        return Completion(
            id: upstream.id,
            created: 0,
            model: upstream.model,
            choices: choices,
            usage: Usage(
                promptTokens: prompt,
                completionTokens: completion,
                totalTokens: prompt + completion
            )
        )
    }

    public func streamCompletion(token: String, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        let body: Data
        do {
            body = try messagesRequestBody(
                model: request.model,
                maxTokens: request.maxTokens ?? 1024,
                messages: request.messages,
                stream: true
            )
        } catch {
            return failedStream(error)
        }

        return streamMessageChunks(token: token, requestBody: body) { chunk in
            guard let text = chunk.textDelta else { return nil }
            return StreamResponseChunk(
                id: "cmpl-\(UUID().uuidString.lowercased())",
                object: "chat.completion.chunk",
                created: Int(Date().timeIntervalSince1970),
                model: request.model,
                systemFingerprint: nil,
                choices: [
                    CompletionChoice(
                        message: nil,
                        text: text,
                        index: UInt16(chunk.index ?? 0),
                        delta: nil,
                        logprobs: nil,
                        finishReason: nil
                    )
                ]
            )
        }
    }

    public func getResponse(token: String, request: ResponseRequest) async throws -> Response {
        let upstream = try await postMessages(
            token: token,
            body: try messagesRequestBody(
                model: request.model,
                maxTokens: request.maxOutputTokens ?? 8192,
                messages: request.input,
                stream: nil
            )
        )

        let content = upstream.content.map { ResponseContent(type: $0.type, text: $0.text) }
        let prompt = upstream.usage.inputTokens
        let completion = upstream.usage.outputTokens

        return Response(
            id: upstream.id,
            model: upstream.model,
            output: [ResponseOutput(type: "message", role: .assistant, content: content)],
            usage: Usage(
                promptTokens: prompt,
                completionTokens: completion,
                totalTokens: prompt + completion
            )
        )
    }

    public func streamResponse(token: String, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        let body: Data
        do {
            body = try messagesRequestBody(
                model: request.model,
                maxTokens: request.maxOutputTokens ?? 1024,
                messages: request.input,
                stream: true
            )
        } catch {
            return failedStream(error)
        }

        return streamMessageChunks(token: token, requestBody: body) { chunk in
            guard let text = chunk.textDelta else {
                if chunk.type == "message_stop" {
                    return ResponseStreamEvent(
                        type: "response.completed",
                        responseID: nil,
                        outputIndex: nil,
                        delta: nil,
                        text: nil
                    )
                }
                return nil
            }

            return ResponseStreamEvent(
                type: "response.output_text.delta",
                responseID: nil,
                outputIndex: chunk.index,
                delta: text,
                text: nil
            )
        }
    }

    private func postMessages(token: String, body: Data) async throws -> AnthropicMessageResponse {
        var headers = baseHeaders(token: token)
        headers["Content-Type"] = "application/json"

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: try endpoint("/v1/messages"),
            headers: headers,
            body: body
        )

        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try JSONDecoder().decode(AnthropicMessageResponse.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    private func streamMessageChunks<T: Sendable>(
        token: String,
        requestBody: Data,
        map: @escaping @Sendable (AnthropicStreamChunk) -> T?
    ) -> AsyncThrowingStream<T, Error> {
        AsyncThrowingStream { continuation in
            Task {
                do {
                    var headers = baseHeaders(token: token)
                    headers["Content-Type"] = "application/json"

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: try endpoint("/v1/messages"),
                        headers: headers,
                        body: requestBody
                    )

                    for try await line in lineStream {
                        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
                        guard trimmed.hasPrefix("data: ") else { continue }

                        let payload = String(trimmed.dropFirst(6))
                        if payload == "[DONE]" {
                            continuation.finish()
                            return
                        }

                        guard let data = payload.data(using: .utf8) else { continue }
                        guard let chunk = try? JSONDecoder().decode(AnthropicStreamChunk.self, from: data) else { continue }

                        if let mapped = map(chunk) {
                            continuation.yield(mapped)
                            if chunk.type == "message_stop" {
                                continuation.finish()
                                return
                            }
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    private func messagesRequestBody(model: String, maxTokens: UInt32, messages: [Message], stream: Bool?) throws -> Data {
        try JSONEncoder().encode(
            AnthropicMessagesRequest(
                model: model,
                maxTokens: maxTokens,
                messages: messages,
                system: nil,
                stream: stream
            )
        )
    }

    private func failedStream<T>(_ error: Error) -> AsyncThrowingStream<T, Error> {
        AsyncThrowingStream { continuation in
            continuation.finish(throwing: error)
        }
    }

    private func baseHeaders(token: String) -> [String: String] {
        [
            "x-api-key": token,
            "anthropic-version": "2023-06-01"
        ]
    }

    private func endpoint(_ path: String) throws -> URL {
        guard let url = URL(string: "\(baseURL.trimmingCharacters(in: CharacterSet(charactersIn: "/")))\(path)") else {
            throw LLMError.invalidRequest("invalid URL")
        }
        return url
    }
}

private struct AnthropicMessagesRequest: Encodable {
    let model: String
    let maxTokens: UInt32
    let messages: [Message]
    let system: String?
    let stream: Bool?

    enum CodingKeys: String, CodingKey {
        case model
        case maxTokens = "max_tokens"
        case messages
        case system
        case stream
    }
}

private struct AnthropicTextContent: Decodable {
    let type: String
    let text: String
}

private struct AnthropicUsage: Decodable {
    let inputTokens: UInt32
    let outputTokens: UInt32

    enum CodingKeys: String, CodingKey {
        case inputTokens = "input_tokens"
        case outputTokens = "output_tokens"
    }
}

private struct AnthropicMessageResponse: Decodable {
    let id: String
    let model: String
    let content: [AnthropicTextContent]
    let stopReason: String?
    let usage: AnthropicUsage

    enum CodingKeys: String, CodingKey {
        case id
        case model
        case content
        case stopReason = "stop_reason"
        case usage
    }
}

private struct AnthropicStreamChunk: Decodable, Sendable {
    let type: String?
    let index: UInt32?
    let delta: AnthropicDelta?
    let contentBlock: AnthropicDelta?
    let text: String?

    enum CodingKeys: String, CodingKey {
        case type
        case index
        case delta
        case contentBlock = "content_block"
        case text
    }

    var textDelta: String? {
        text ?? delta?.text ?? contentBlock?.text
    }
}

private struct AnthropicDelta: Decodable, Sendable {
    let text: String?
}
