import Foundation
import LLMKit

public protocol ChatTransport: Sendable {
    var providerId: String { get }
    func listModels() async -> [LangModel]
    func getResponse(
        modelId: String,
        messages: [ConversationTurn],
        tools: [any Tool]
    ) async throws -> Response
    func streamResponse(
        modelId: String,
        messages: [ConversationTurn],
        tools: [any Tool]
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
        messages: [ConversationTurn],
        tools: [any Tool]
    ) -> AsyncThrowingStream<ConversationTurn, Error> {
        let sortedMessages = messages.sorted { $0.createdAt < $1.createdAt }

        var request = ResponseRequest(
            model: modelId,
            input: sortedMessages.map { Message(role: mapRole($0.role), content: $0.text) }
        ).stream()
        if !tools.isEmpty {
            request = request.tools(tools.map { $0.asLLM() }).toolChoice(.auto)
        }

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

    public func getResponse(
        modelId: String,
        messages: [ConversationTurn],
        tools: [any Tool]
    ) async throws -> Response {
        let sortedMessages = messages.sorted { $0.createdAt < $1.createdAt }
        var request = ResponseRequest(
            model: modelId,
            input: sortedMessages.map { Message(role: mapRole($0.role), content: $0.text) }
        )
        if !tools.isEmpty {
            request = request.tools(tools.map { $0.asLLM() }).toolChoice(.auto)
        }
        return try await api.getResponse(token: token, request: request)
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
    case maxToolIterationsExceeded
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

    public func addMessage(tools: [any Tool], next: ConversationTurn) -> AsyncThrowingStream<
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

        if tools.isEmpty {
            let upstream = transport.streamResponse(modelId: modelId, messages: messages, tools: tools)

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

        return AsyncThrowingStream { continuation in
            Task {
                do {
                    var workingMessages = messages
                    let assistant = ConversationTurn(
                        role: .assistant,
                        text: "",
                        createdAt: .now,
                        modelIdentifier: modelId
                    )
                    var invocations: [ToolInvocation] = []

                    for _ in 0..<8 {
                        let response = try await transport.getResponse(
                            modelId: modelId,
                            messages: workingMessages,
                            tools: tools
                        )

                        let outputText = response.outputText
                        if !outputText.isEmpty {
                            assistant.text += outputText
                        }

                        let calls = Self.extractFunctionCalls(from: response)
                        if calls.isEmpty {
                            assistant.toolInvocations = invocations
                            self.appendMessage(assistant)
                            continuation.yield(assistant)
                            continuation.finish()
                            return
                        }

                        for call in calls {
                            let startedAt = Date.now
                            var invocation = ToolInvocation(
                                name: call.name,
                                argumentsJSON: call.arguments,
                                status: .running,
                                startedAt: startedAt
                            )

                            let result: String
                            if let tool = tools.first(where: { $0.id() == call.name }) {
                                let parsedArgs = Self.parseToolArgs(argumentsJSON: call.arguments)
                                result = await tool.execute(args: parsedArgs)
                            } else {
                                result = "Error: Unknown tool '\(call.name)'."
                            }

                            invocation.status = result.hasPrefix("Error:") ? .failed : .completed
                            invocation.resultJSON = result
                            invocation.endedAt = .now
                            invocations.append(invocation)

                            let toolMessage = ConversationTurn(
                                role: .tool,
                                text: result,
                                createdAt: .now,
                                modelIdentifier: modelId
                            )
                            self.appendMessage(toolMessage)
                            workingMessages.append(toolMessage)
                        }
                    }

                    continuation.finish(throwing: ChatStreamError.maxToolIterationsExceeded)
                } catch {
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

    private static func extractFunctionCalls(from response: Response) -> [ModelFunctionCall] {
        response.output.compactMap { output in
            guard output.type == "function_call", let name = output.name else {
                return nil
            }
            return ModelFunctionCall(
                name: name,
                arguments: output.arguments ?? "{}",
                callID: output.callID
            )
        }
    }

    private static func parseToolArgs(argumentsJSON: String) -> [ToolArg] {
        guard let data = argumentsJSON.data(using: .utf8),
            let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return []
        }

        return object.map { key, value in
            let valueString: String
            if let stringValue = value as? String {
                valueString = stringValue
            } else {
                switch value {
                case is NSNumber, is NSNull:
                    valueString = String(describing: value)
                default:
                    if let jsonData = try? JSONSerialization.data(withJSONObject: value),
                        let jsonString = String(data: jsonData, encoding: .utf8)
                    {
                        valueString = jsonString
                    } else {
                        valueString = String(describing: value)
                    }
                }
            }
            return ToolArg(name: key, value: valueString)
        }
    }
}

private struct ModelFunctionCall {
    let name: String
    let arguments: String
    let callID: String?
}
