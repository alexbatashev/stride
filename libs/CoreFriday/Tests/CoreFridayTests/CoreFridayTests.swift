import Testing
@testable import CoreFriday
import LLMKit

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
    let chat = ChatStream(transports: [transport])
    await chat.setModel(providerId: "test-provider", modelId: "test-model")

    let stream = await chat.addMessage(
        tools: [JSTool()],
        next: ConversationTurn(role: .user, text: "run js", createdAt: .now)
    )

    var last: ConversationTurn?
    for try await partial in stream {
        last = partial
    }

    let assistant = try #require(last)
    #expect(assistant.text.contains("Tool output: hello from tool"))
    #expect(assistant.toolInvocations.count == 1)
    #expect(assistant.toolInvocations[0].name == "execute_js")
    #expect(assistant.toolInvocations[0].status == .completed)
}

private final class ScriptedToolTransport: ChatTransport, @unchecked Sendable {
    let providerId = "test-provider"
    private var didRequestTool = false

    func listModels() async -> [LangModel] { [] }

    func getResponse(
        modelId _: String,
        messages: [ConversationTurn],
        tools _: [any CoreFriday.Tool]
    ) async throws -> Response {
        if !didRequestTool {
            didRequestTool = true
            return Response(
                id: "r1",
                model: "test-model",
                output: [
                    ResponseOutput(
                        type: "function_call",
                        name: "execute_js",
                        arguments: #"{"code":"console.log('hello from tool')","timeout":5}"#
                    )
                ],
                usage: nil
            )
        }

        let toolMessage = messages.last(where: { $0.role == .tool })?.text ?? ""
        return Response(
            id: "r2",
            model: "test-model",
            output: [
                ResponseOutput(
                    type: "message",
                    role: .assistant,
                    content: [ResponseContent(type: "output_text", text: "Tool output: \(toolMessage)")]
                )
            ],
            usage: nil
        )
    }

    func streamResponse(
        modelId _: String,
        messages _: [ConversationTurn],
        tools _: [any CoreFriday.Tool]
    ) -> AsyncThrowingStream<ConversationTurn, Error> {
        AsyncThrowingStream { continuation in
            continuation.finish()
        }
    }
}
