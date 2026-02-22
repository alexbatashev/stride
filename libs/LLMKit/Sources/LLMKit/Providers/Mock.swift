import Foundation

public struct Mock: Sendable {
    public init() {}

    public static func api() -> API {
        .mock(Mock())
    }

    public func listModels(token _: String) async throws -> [ModelDesc] {
        [
            ModelDesc(id: "mock-model", object: "model", created: 0, ownedBy: "mock-owner")
        ]
    }

    public func getModel(token _: String, model: String) async throws -> ModelDesc {
        ModelDesc(id: model, object: "model", created: 0, ownedBy: "mock-owner")
    }

    public func getCompletion(token _: String, request: CompletionRequest) async throws -> Completion {
        Completion(
            id: "mock-completion-id",
            created: 0,
            model: "mock-model",
            choices: [
                CompletionChoice(
                    message: Message(role: .assistant, content: "Echo: \(request.messages)"),
                    text: "This is a mock completion.",
                    index: 0,
                    delta: nil,
                    logprobs: nil,
                    finishReason: "stop"
                )
            ],
            usage: Usage(promptTokens: 0, completionTokens: 0, totalTokens: 0)
        )
    }

    public func streamCompletion(token _: String, request _: CompletionRequest) -> AsyncThrowingStream<StreamResponseChunk, Error> {
        AsyncThrowingStream { continuation in
            let chunk = StreamResponseChunk(
                id: "mock-stream-id",
                object: "mock.stream",
                created: 0,
                model: "mock-model",
                systemFingerprint: nil,
                choices: [
                    CompletionChoice(
                        message: nil,
                        text: "Partial mock stream response.",
                        index: 0,
                        delta: Delta(content: "Partial mock stream response."),
                        logprobs: nil,
                        finishReason: "stop"
                    )
                ]
            )
            continuation.yield(chunk)
            continuation.finish()
        }
    }
}
