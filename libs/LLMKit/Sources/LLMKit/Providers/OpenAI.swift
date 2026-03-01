import Foundation

public struct OpenAI: Sendable {
    public let baseURL: String

    public init(baseURL: String) {
        self.baseURL = baseURL
    }

    public static func api(baseURL: String) -> API {
        .openAI(OpenAI(baseURL: baseURL))
    }

    public func listModels(token: String) async throws -> [ModelDesc] {
        struct ModelListResponse: Decodable {
            let object: String
            let data: [ModelDesc]
        }

        let url = try endpoint("/v1/models")
        let (response, data) = try await HTTPClient.request(
            method: "GET",
            url: url,
            headers: ["Authorization": "Bearer \(token)"]
        )
        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try jsonDecoder.decode(ModelListResponse.self, from: data).data
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getEmbeddings(token: String, input: String, model: String) async throws -> EmbeddingResponse {
        struct RequestData: Encodable {
            let input: String
            let model: String
        }

        let url = try endpoint("/v1/embeddings")
        let body = try jsonEncoder.encode(RequestData(input: input, model: model))
        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: url,
            headers: [
                "Content-Type": "application/json",
                "Authorization": "Bearer \(token)"
            ],
            body: body
        )
        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try jsonDecoder.decode(EmbeddingResponse.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getModel(token: String, model: String) async throws -> ModelDesc {
        let url = try endpoint("/v1/models/\(model)")
        let (response, data) = try await HTTPClient.request(
            method: "GET",
            url: url,
            headers: ["Authorization": "Bearer \(token)"]
        )
        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try jsonDecoder.decode(ModelDesc.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func getCompletion(token: String, request: CompletionRequest) async throws -> Completion {
        let url = try endpoint("/v1/chat/completions")
        let body = try jsonEncoder.encode(request)

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: url,
            headers: [
                "Content-Type": "application/json",
                "Authorization": "Bearer \(token)"
            ],
            body: body
        )
        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try jsonDecoder.decode(Completion.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func streamCompletion(token: String, request: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        AsyncThrowingStream { continuation in
            Task {
                do {
                    let url = try endpoint("/v1/chat/completions")
                    let body = try jsonEncoder.encode(request)

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: url,
                        headers: [
                            "Content-Type": "application/json",
                            "Authorization": "Bearer \(token)"
                        ],
                        body: body
                    )

                    for try await line in lineStream {
                        guard line.hasPrefix("data: ") else { continue }
                        let data = String(line.dropFirst(6))
                        if data == "[DONE]" {
                            continuation.finish()
                            return
                        }

                        do {
                            let chunk = try jsonDecoder.decode(StreamResponseChunk.self, from: Data(data.utf8))
                            continuation.yield(chunk)
                        } catch {
                            throw LLMError.parsingError(String(describing: error))
                        }
                    }
                    continuation.finish()
                } catch {
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    public func getResponse(token: String, request: ResponseRequest) async throws -> Response {
        let url = try endpoint("/v1/responses")
        let body = try jsonEncoder.encode(request)

        let (response, data) = try await HTTPClient.request(
            method: "POST",
            url: url,
            headers: [
                "Content-Type": "application/json",
                "Authorization": "Bearer \(token)"
            ],
            body: body
        )
        guard (200..<300).contains(response.statusCode) else {
            throw LLMError.serverError(response.statusCode)
        }

        do {
            return try jsonDecoder.decode(Response.self, from: data)
        } catch {
            throw LLMError.parsingError(String(describing: error))
        }
    }

    public func streamResponse(token: String, request: ResponseRequest) -> AsyncThrowingStream<ResponseStreamEvent, Error> {
        AsyncThrowingStream { continuation in
            Task {
                do {
                    let url = try endpoint("/v1/responses")
                    let body = try jsonEncoder.encode(request)

                    let lineStream = HTTPClient.streamLines(
                        method: "POST",
                        url: url,
                        headers: [
                            "Content-Type": "application/json",
                            "Authorization": "Bearer \(token)"
                        ],
                        body: body
                    )

                    for try await line in lineStream {
                        guard line.hasPrefix("data: ") else { continue }
                        let data = String(line.dropFirst(6))
                        if data == "[DONE]" {
                            continuation.finish()
                            return
                        }

                        do {
                            let event = try jsonDecoder.decode(ResponseStreamEvent.self, from: Data(data.utf8))
                            continuation.yield(event)
                            if event.type == "response.completed" {
                                continuation.finish()
                                return
                            }
                        } catch {
                            throw LLMError.parsingError(String(describing: error))
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

private let jsonDecoder: JSONDecoder = {
    JSONDecoder()
}()

private let jsonEncoder: JSONEncoder = {
    JSONEncoder()
}()
