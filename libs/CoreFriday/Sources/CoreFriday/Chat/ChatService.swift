import Foundation
import LLMKit

public protocol ChatTransport: Sendable {
    var providerId: String { get }
    func listModels() async -> [LangModel]
    func getResponse(
        modelId: String,
        messages: [ChatMessage],
        tools: [any Tool]
    ) async throws -> Response
    func streamResponse(
        modelId: String,
        messages: [ChatMessage],
        tools: [any Tool]
    ) -> AsyncThrowingStream<ChatMessage, Error>
}

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
        messages: [ChatMessage],
        tools: [any Tool]
    ) -> AsyncThrowingStream<ChatMessage, Error> {
        var request = ResponseRequest(
            model: modelId,
            input: messages.map { Message(role: mapRole($0.role), content: $0.content) }
        ).stream()
        if !tools.isEmpty {
            request = request.tools(tools.map { $0.asLLM() }).toolChoice(.auto)
        }

        let upstream = api.streamResponse(token: token, request: request)

        return AsyncThrowingStream { continuation in
            Task {
                var streamedToolCalls: [[String: String]] = []
                let responseMessage = ChatMessage(
                    id: UUID(),
                    threadId: messages.last?.threadId ?? UUID(),
                    userId: nil,
                    parentId: messages.last?.id,
                    providerId: providerId,
                    modelId: modelId,
                    modelName: modelId,
                    role: .assistant,
                    thinking: nil,
                    content: "",
                    createdAt: .now,
                    updatedAt: .now,
                    isDone: false
                )

                do {
                    for try await event in upstream {
                        if event.type == "response.function_call", let name = event.name {
                            var entry: [String: String] = [
                                "name": name,
                                "arguments": event.arguments ?? "{}",
                            ]
                            if let callID = event.callID {
                                entry["callID"] = callID
                            }
                            streamedToolCalls.append(entry)
                            responseMessage.toolCall = Self.jsonString(from: streamedToolCalls)
                            responseMessage.updatedAt = .now
                            continuation.yield(responseMessage)
                            continue
                        }

                        let token = event.delta ?? event.text ?? ""
                        guard !token.isEmpty else { continue }

                        let isThinking =
                            event.type.lowercased().contains("reasoning")
                            || event.type.lowercased().contains("thinking")
                        if isThinking {
                            responseMessage.thinking = (responseMessage.thinking ?? "") + token
                        } else {
                            responseMessage.content += token
                        }
                        responseMessage.updatedAt = .now
                        continuation.yield(responseMessage)
                    }
                    responseMessage.isDone = true
                    responseMessage.updatedAt = .now
                    continuation.finish()
                } catch {
                    responseMessage.isDone = true
                    responseMessage.updatedAt = .now
                    continuation.yield(responseMessage)
                    continuation.finish(throwing: error)
                }
            }
        }
    }

    public func getResponse(
        modelId: String,
        messages: [ChatMessage],
        tools: [any Tool]
    ) async throws -> Response {
        var request = ResponseRequest(
            model: modelId,
            input: messages.map { Message(role: mapRole($0.role), content: $0.content) }
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

    private static func jsonString(from dictionaries: [[String: String]]) -> String? {
        guard JSONSerialization.isValidJSONObject(dictionaries),
            let data = try? JSONSerialization.data(withJSONObject: dictionaries),
            let text = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return text
    }
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

    private var hasLoadedStorage = false
    private var messages: [ChatMessage]

    public init(
        transports: [any ChatTransport],
        storage: ChatStorage = NullChatStorage(),
    ) {
        self.transports = transports.sorted { $0.providerId < $1.providerId }
        self.storage = storage
        self.messages = []
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

    public func getMessages() async -> [ChatMessage] {
        await ensureMessagesLoaded()
        return self.messages
    }

    public func setModel(providerId: String, modelId: String) {
        // TODO check provider with the requested providerId exists
        self.providerId = providerId
        // Model ID is stored as-is as the provider will return an error if the model does not exist during request
        self.modelId = modelId
    }

    public func addMessage(tools: [any Tool], next: ChatMessage) async -> AsyncThrowingStream<
        ChatMessage, Error
    > {
        await ensureMessagesLoaded()
        await appendMessage(next)

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
            let upstream = transport.streamResponse(
                modelId: modelId, messages: messages, tools: tools)

            return AsyncThrowingStream { continuation in
                Task {
                    var latest: ChatMessage?

                    do {
                        for try await partial in upstream {
                            latest = partial
                            continuation.yield(partial)
                        }

                        if let latest {
                            await self.appendMessage(latest)
                        }
                        continuation.finish()
                    } catch {
                        if let latest {
                            await self.appendMessage(latest)
                        }
                        continuation.finish(throwing: error)
                    }
                }
            }
        }

        return AsyncThrowingStream { continuation in
            Task {
                var workingMessages = messages
                let threadId = workingMessages.last?.threadId ?? next.threadId

                for _ in 0..<8 {
                    let upstream = transport.streamResponse(
                        modelId: modelId,
                        messages: workingMessages,
                        tools: tools
                    )
                    var latest: ChatMessage?

                    do {
                        for try await partial in upstream {
                            latest = partial
                            continuation.yield(partial)
                        }
                    } catch {
                        if let latest {
                            latest.isDone = true
                            latest.updatedAt = .now
                            await self.appendMessage(latest)
                        }
                        continuation.finish(throwing: error)
                        return
                    }

                    guard let assistant = latest else {
                        continuation.finish()
                        return
                    }

                    assistant.isDone = true
                    assistant.updatedAt = .now
                    await self.appendMessage(assistant)
                    workingMessages.append(assistant)

                    let calls = Self.extractFunctionCalls(fromToolCallJSON: assistant.toolCall)
                    if calls.isEmpty {
                        continuation.finish()
                        return
                    }

                    var toolResultEntries: [[String: String]] = []
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
                        toolResultEntries.append(
                            Self.toolResultDictionary(from: call, invocation: invocation))

                        let toolMessage = ChatMessage(
                            id: UUID(),
                            threadId: threadId,
                            userId: nil,
                            parentId: assistant.id,
                            providerId: providerId,
                            modelId: modelId,
                            modelName: modelId,
                            role: .tool,
                            thinking: nil,
                            content: result,
                            toolCall: nil,
                            toolResult: assistant.toolResult,
                            createdAt: .now,
                            updatedAt: .now,
                            isDone: true
                        )
                        await self.appendMessage(toolMessage)
                        workingMessages.append(toolMessage)
                    }

                    assistant.toolResult = Self.jsonString(from: toolResultEntries)
                    assistant.updatedAt = .now
                }

                continuation.finish(throwing: ChatStreamError.maxToolIterationsExceeded)
            }
        }
    }

    private static func failedStream(_ error: Error) -> AsyncThrowingStream<ChatMessage, Error> {
        AsyncThrowingStream { continuation in
            continuation.finish(throwing: error)
        }
    }

    private func ensureMessagesLoaded() async {
        guard !hasLoadedStorage else {
            return
        }
        hasLoadedStorage = true
        messages = await storage.listMessages().sorted { $0.createdAt < $1.createdAt }
    }

    private func appendMessage(_ message: ChatMessage) async {
        messages.append(message)
        await storage.appendMessage(message: message)
    }

    private static func extractFunctionCalls(fromToolCallJSON raw: String?) -> [ModelFunctionCall] {
        guard let raw,
            let data = raw.data(using: .utf8),
            let list = try? JSONSerialization.jsonObject(with: data) as? [[String: String]]
        else {
            return []
        }

        return list.compactMap { entry in
            guard let name = entry["name"] else {
                return nil
            }
            return ModelFunctionCall(
                name: name,
                arguments: entry["arguments"] ?? "{}",
                callID: entry["callID"]
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

    private static func toolResultDictionary(
        from call: ModelFunctionCall,
        invocation: ToolInvocation
    ) -> [String: String] {
        var dictionary: [String: String] = [
            "name": call.name,
            "status": invocation.status.rawValue,
            "result": invocation.resultJSON ?? "",
        ]
        if let callID = call.callID {
            dictionary["callID"] = callID
        }
        return dictionary
    }

    private static func jsonString(from dictionaries: [[String: String]]) -> String? {
        guard JSONSerialization.isValidJSONObject(dictionaries),
            let data = try? JSONSerialization.data(withJSONObject: dictionaries),
            let text = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return text
    }

}

private struct ModelFunctionCall {
    let name: String
    let arguments: String
    let callID: String?
}
