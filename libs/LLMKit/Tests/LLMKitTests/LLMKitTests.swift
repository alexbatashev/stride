import Testing
@testable import LLMKit

@Test func completionRequestToolsEncoding() throws {
    let request = CompletionRequest(
        model: "test-model",
        messages: [Message(role: .user, content: "Hello")]
    )
    .frequencyPenalty(1.0)
    .topP(0.2)
    .tools([
        Function(description: "Test function", name: "test").asTool()
    ])
    .toolChoice(.required)

    let data = try JSONEncoder().encode(request)
    let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])

    #expect(object["model"] as? String == "test-model")
    #expect(object["frequency_penalty"] as? Double == 1.0)
    #expect(object["top_p"] as? Double == 0.2)
    #expect(object["tool_choice"] as? String == "required")

    let tools = try #require(object["tools"] as? [[String: Any]])
    let function = try #require(tools.first?["function"] as? [String: Any])
    #expect(function["name"] as? String == "test")
}

@Test func namedToolChoiceEncoding() throws {
    let request = CompletionRequest(model: "x", messages: []).toolChoice(FunctionRef(name: "lookup"))

    let data = try JSONEncoder().encode(request)
    let object = try #require(try JSONSerialization.jsonObject(with: data) as? [String: Any])
    let toolChoice = try #require(object["tool_choice"] as? [String: Any])
    let function = try #require(toolChoice["function"] as? [String: Any])

    #expect(toolChoice["type"] as? String == "function")
    #expect(function["name"] as? String == "lookup")
}

@Test func embeddingResponseDecodesArrayData() throws {
    let json = """
    {
      "object": "list",
      "model": "text-embedding-3-small",
      "data": [
        {
          "object": "embedding",
          "index": 0,
          "embedding": [0.1, 0.2]
        }
      ],
      "usage": {
        "prompt_tokens": 3,
        "completion_tokens": 0,
        "total_tokens": 3
      }
    }
    """.data(using: .utf8)!

    let decoded = try JSONDecoder().decode(EmbeddingResponse.self, from: json)
    #expect(decoded.data.index == 0)
    #expect(decoded.data.embedding.count == 2)
}

@Test func apiRejectsNonStreamingCompletionRequestWhenStreamTrue() async throws {
    let api = Mock.api()
    let request = CompletionRequest(model: "x", messages: []).stream()

    await #expect(throws: LLMError.self) {
        _ = try await api.getCompletion(token: "", request: request)
    }
}

@Test func mockProviderListModels() async throws {
    let api = Mock.api()
    let models = try await api.listModels(token: "")

    #expect(models.count == 1)
    #expect(models.first?.id == "mock-model")
    #expect(models.first?.ownedBy == "mock-owner")
}

@Test func mockProviderGetModelByName() async throws {
    let api = Mock.api()
    let model = try await api.getModel(token: "", modelName: "my-model")

    #expect(model.id == "my-model")
    #expect(model.object == "model")
}

@Test func mockProviderCompletionReturnsAssistantMessage() async throws {
    let api = Mock.api()
    let request = CompletionRequest(
        model: "test-model",
        messages: [Message(role: .user, content: "hello")]
    )

    let completion = try await api.getCompletion(token: "", request: request)
    #expect(completion.model == "mock-model")
    #expect(completion.choices.count == 1)
    #expect(completion.choices[0].finishReason == "stop")
    #expect(completion.choices[0].message?.role == .assistant)
    #expect(completion.choices[0].text == "This is a mock completion.")
}

@Test func mockProviderStreamCompletionYieldsSingleChunk() async throws {
    let api = Mock.api()
    let stream = api.streamCompletion(
        token: "",
        request: CompletionRequest(model: "test-model", messages: [Message(role: .user, content: "hello")])
    )

    var chunks: [StreamResponseChunk] = []
    for try await chunk in stream {
        chunks.append(chunk)
    }

    #expect(chunks.count == 1)
    #expect(chunks[0].id == "mock-stream-id")
    #expect(chunks[0].choices.first?.delta?.content == "Partial mock stream response.")
}

@Test func mockProviderEmbeddingsAreNotImplemented() async throws {
    let api = Mock.api()

    await #expect(throws: LLMError.self) {
        _ = try await api.getEmbeddings(token: "", input: "x", model: "x")
    }
}
