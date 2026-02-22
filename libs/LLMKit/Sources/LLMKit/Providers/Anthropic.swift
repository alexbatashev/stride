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
        struct AnthropicCompletionRequest: Encodable {
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

        struct AnthropicTextContent: Decodable {
            let type: String
            let text: String
        }

        struct AnthropicUsage: Decodable {
            let inputTokens: UInt32
            let outputTokens: UInt32

            enum CodingKeys: String, CodingKey {
                case inputTokens = "input_tokens"
                case outputTokens = "output_tokens"
            }
        }

        struct AnthropicCompletionResponse: Decodable {
            let id: String
            let type: String
            let role: String
            let model: String
            let content: [AnthropicTextContent]
            let stopReason: String?
            let stopSequence: String?
            let usage: AnthropicUsage

            enum CodingKeys: String, CodingKey {
                case id
                case type
                case role
                case model
                case content
                case stopReason = "stop_reason"
                case stopSequence = "stop_sequence"
                case usage
            }
        }

        let body = try JSONEncoder().encode(
            AnthropicCompletionRequest(
                model: request.model,
                maxTokens: request.maxTokens ?? 8192,
                messages: request.messages,
                system: nil,
                stream: nil
            )
        )

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
            let anthropic = try JSONDecoder().decode(AnthropicCompletionResponse.self, from: data)
            let choices = anthropic.content.enumerated().map { index, content in
                CompletionChoice(
                    message: nil,
                    text: content.text,
                    index: UInt16(index),
                    delta: nil,
                    logprobs: nil,
                    finishReason: anthropic.stopReason
                )
            }

            let prompt = anthropic.usage.inputTokens
            let completion = anthropic.usage.outputTokens
            return Completion(
                id: anthropic.id,
                created: 0,
                model: anthropic.model,
                choices: choices,
                usage: Usage(
                    promptTokens: prompt,
                    completionTokens: completion,
                    totalTokens: prompt + completion
                )
            )
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func streamCompletion(token: String, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        struct AnthropicCompletionRequest: Encodable {
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

        struct AnthropicTextContent: Codable {
            let type: String
            let text: String
        }

        struct StreamChunk: Decodable {
            let index: UInt32?
            let delta: AnthropicTextContent?
            let contentBlock: AnthropicTextContent?
            let text: String?

            enum CodingKeys: String, CodingKey {
                case index
                case delta
                case contentBlock = "content_block"
                case text
            }
        }

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    let body = try JSONEncoder().encode(
                        AnthropicCompletionRequest(
                            model: request.model,
                            maxTokens: request.maxTokens ?? 1024,
                            messages: request.messages,
                            system: nil,
                            stream: true
                        )
                    )

                    var headers = baseHeaders(token: token)
                    headers["Content-Type"] = "application/json"

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: try endpoint("/v1/messages"),
                        headers: headers,
                        body: body
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
                        guard let chunk = try? JSONDecoder().decode(StreamChunk.self, from: data) else { continue }

                        let text = chunk.text ?? chunk.delta?.text ?? chunk.contentBlock?.text
                        guard let text else { continue }

                        let responseChunk = StreamResponseChunk(
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
                        continuation.yield(responseChunk)
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
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
