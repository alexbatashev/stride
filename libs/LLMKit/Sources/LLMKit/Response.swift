import Foundation

public struct ResponseRequest: Codable, Equatable, Sendable {
    public var model: String
    public var input: [Message]
    public var isStream: Bool?
    public var maxOutputTokens: UInt32?
    public var temperature: Float?
    public var topP: Float?
    public var tools: [Tool]?
    public var toolChoice: ToolChoice?

    enum CodingKeys: String, CodingKey {
        case model
        case input
        case isStream = "stream"
        case maxOutputTokens = "max_output_tokens"
        case temperature
        case topP = "top_p"
        case tools
        case toolChoice = "tool_choice"
    }

    public init(model: String, input: [Message]) {
        self.model = model
        self.input = input
    }

    public func stream() -> Self {
        var copy = self
        copy.isStream = true
        return copy
    }

    public func maxOutputTokens(_ value: UInt32) -> Self {
        var copy = self
        copy.maxOutputTokens = value
        return copy
    }

    public func temperature(_ value: Float) -> Self {
        var copy = self
        copy.temperature = value
        return copy
    }

    public func topP(_ value: Float) -> Self {
        var copy = self
        copy.topP = value
        return copy
    }

    public func tools(_ value: [Tool]) -> Self {
        var copy = self
        copy.tools = value
        return copy
    }

    public func toolChoice(_ choice: ToolChoice) -> Self {
        var copy = self
        copy.toolChoice = choice
        return copy
    }

    public func toolChoice(_ choice: UnnamedToolChoice) -> Self {
        toolChoice(.unnamed(choice))
    }

    public func toolChoice(_ function: FunctionRef) -> Self {
        toolChoice(.named(type: "function", function: function))
    }
}

public struct Response: Codable, Equatable, Sendable {
    public var id: String
    public var model: String
    public var output: [ResponseOutput]
    public var usage: Usage?

    public init(id: String, model: String, output: [ResponseOutput], usage: Usage?) {
        self.id = id
        self.model = model
        self.output = output
        self.usage = usage
    }

    public var outputText: String {
        output
            .flatMap(\.content)
            .compactMap(\.text)
            .joined()
    }
}

public struct ResponseOutput: Codable, Equatable, Sendable {
    public var type: String
    public var role: Role?
    public var content: [ResponseContent]

    public init(type: String, role: Role?, content: [ResponseContent]) {
        self.type = type
        self.role = role
        self.content = content
    }
}

public struct ResponseContent: Codable, Equatable, Sendable {
    public var type: String
    public var text: String?

    public init(type: String, text: String?) {
        self.type = type
        self.text = text
    }
}

public struct ResponseStreamEvent: Codable, Equatable, Sendable {
    public var type: String
    public var responseID: String?
    public var outputIndex: UInt32?
    public var delta: String?
    public var text: String?

    enum CodingKeys: String, CodingKey {
        case type
        case responseID = "response_id"
        case outputIndex = "output_index"
        case delta
        case text
    }

    public init(type: String, responseID: String?, outputIndex: UInt32?, delta: String?, text: String?) {
        self.type = type
        self.responseID = responseID
        self.outputIndex = outputIndex
        self.delta = delta
        self.text = text
    }
}
