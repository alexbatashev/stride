import LLMKit
import Testing

@testable import CoreFriday

@Test func jsToolReturnsConcatenatedConsoleLogs() async throws {
    let tool = JSTool()
    let output = await tool.execute(
        args: [
            ToolArg(name: "code", value: "console.log('hello'); console.log('world')"),
            ToolArg(name: "timeout", value: "5"),
        ]
    )

    #expect(output == "hello\nworld\n")
}

@Test func jsToolRejectsTimeoutOverThreeMinutes() async throws {
    let tool = JSTool()
    let output = await tool.execute(
        args: [
            ToolArg(name: "code", value: "console.log('x')"),
            ToolArg(name: "timeout", value: "181"),
        ]
    )

    #expect(output == "Error: 'timeout' must not exceed 180 seconds.")
}

@Test func jsToolStopsLongRunningCodeOnTimeout() async throws {
    let tool = JSTool()
    let output = await tool.execute(
        args: [
            ToolArg(name: "code", value: "while (true) {}"),
            ToolArg(name: "timeout", value: "1"),
        ]
    )

    #expect(output.hasPrefix("Error:"))
}

@Test func chatStreamExecutesModelFunctionCalls() async throws {
    let transport = ScriptedToolTransport()
    let chat = ChatService(transports: [transport])
    await chat.setModel(providerId: "test-provider", modelId: "test-model")

    let stream = await chat.addMessage(
        tools: [JSTool()],
        next: ChatMessage(role: .user, content: "run js")
    )

    var yieldedMessages: [ChatMessage] = []
    for try await partial in stream {
        yieldedMessages.append(partial)
    }

    // Final message is the assistant's text response
    let finalAssistant = try #require(yieldedMessages.last)
    #expect(finalAssistant.content.contains("Tool output: hello from tool"))

    // First yielded message had the tool call; after execution its toolResult is populated
    let toolCallMessage = try #require(yieldedMessages.first)
    let toolResultJSON = try #require(toolCallMessage.toolResult)
    let toolResultData = try #require(toolResultJSON.data(using: .utf8))
    let toolResults = try #require(
        try JSONSerialization.jsonObject(with: toolResultData) as? [[String: String]]
    )
    #expect(toolResults.count == 1)
    #expect(toolResults[0]["name"] == "execute_js")
    #expect(toolResults[0]["status"] == "completed")
}

private final class ScriptedToolTransport: ChatTransport, @unchecked Sendable {
    let providerId = "test-provider"

    func listModels() async -> [LangModel] { [] }

    func getResponse(
        modelId _: String,
        messages _: [ChatMessage],
        tools _: [any CoreFriday.Tool]
    ) async throws -> Response {
        Response(id: "", model: "", output: [], usage: nil)
    }

    func streamResponse(
        modelId _: String,
        messages: [ChatMessage],
        tools _: [any CoreFriday.Tool]
    ) -> AsyncThrowingStream<ChatMessage, Error> {
        let hasToolResult = messages.last?.role == .tool
        let toolResultContent = messages.last?.content ?? ""

        return AsyncThrowingStream { continuation in
            if hasToolResult {
                let msg = ChatMessage(role: .assistant, content: "Tool output: \(toolResultContent)")
                msg.isDone = true
                continuation.yield(msg)
            } else {
                let msg = ChatMessage(role: .assistant, content: "")
                msg.toolCall =
                    #"[{"name":"execute_js","arguments":"{\"code\":\"console.log('hello from tool')\",\"timeout\":5}"}]"#
                msg.isDone = true
                continuation.yield(msg)
            }
            continuation.finish()
        }
    }
}
