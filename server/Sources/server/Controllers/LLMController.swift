import Vapor

struct ChatMessage: Content {
    let role: String
    let content: String
}

struct ChatCompletionRequest: Content {
    let model: String
    let messages: [ChatMessage]
}

struct ChatCompletionChoice: Content {
    let index: Int
    let message: ChatMessage
    let finishReason: String

    enum CodingKeys: String, CodingKey {
        case index
        case message
        case finishReason = "finish_reason"
    }
}

struct ChatCompletionUsage: Content {
    let promptTokens: Int
    let completionTokens: Int
    let totalTokens: Int

    enum CodingKeys: String, CodingKey {
        case promptTokens = "prompt_tokens"
        case completionTokens = "completion_tokens"
        case totalTokens = "total_tokens"
    }
}

struct ChatCompletionResponse: Content {
    let id: String
    let object: String
    let created: Int
    let model: String
    let choices: [ChatCompletionChoice]
    let usage: ChatCompletionUsage
}

struct ModelObject: Content {
    let id: String
    let object: String
    let created: Int
    let ownedBy: String

    enum CodingKeys: String, CodingKey {
        case id
        case object
        case created
        case ownedBy = "owned_by"
    }
}

struct ModelListResponse: Content {
    let object: String
    let data: [ModelObject]
}

struct LLMController: RouteCollection {
    func boot(routes: any RoutesBuilder) throws {
        let apiKeyProtected = routes.grouped(APIKeyAuthenticator(), APIKeyUser.guardMiddleware())
        let llm = apiKeyProtected.grouped("api", "llm")
        llm.get("models", use: models)
        llm.post("chat", "completions", use: chatCompletions)
    }

    @Sendable
    func models(req: Request) async throws -> ModelListResponse {
        ModelListResponse(
            object: "list",
            data: [
                ModelObject(
                    id: "friday-1",
                    object: "model",
                    created: 1_700_000_000,
                    ownedBy: "friday"
                )
            ]
        )
    }

    @Sendable
    func chatCompletions(req: Request) async throws -> ChatCompletionResponse {
        let body = try req.content.decode(ChatCompletionRequest.self)

        let lastUserMessage = body.messages.last(where: { $0.role == "user" })?.content
            ?? "(no user message)"

        return ChatCompletionResponse(
            id: "chatcmpl-\(UUID().uuidString)",
            object: "chat.completion",
            created: Int(Date().timeIntervalSince1970),
            model: body.model,
            choices: [
                ChatCompletionChoice(
                    index: 0,
                    message: ChatMessage(role: "assistant", content: "Echo: \(lastUserMessage)"),
                    finishReason: "stop"
                )
            ],
            usage: ChatCompletionUsage(promptTokens: 0, completionTokens: 0, totalTokens: 0)
        )
    }
}
