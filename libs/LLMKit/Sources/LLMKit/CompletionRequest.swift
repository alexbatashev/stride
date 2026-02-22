import Foundation

public enum ResponseFormatType: String, Codable, Equatable, Sendable {
    case text
    case jsonObject = "json_object"
}

public struct ResponseFormat: Codable, Equatable, Sendable {
    public var type: ResponseFormatType

    public init(type: ResponseFormatType) {
        self.type = type
    }
}

public enum ToolType: String, Codable, Equatable, Sendable {
    case function
}

public struct Tool: Codable, Equatable, Sendable {
    public var type: ToolType
    public var function: Function

    public init(type: ToolType = .function, function: Function) {
        self.type = type
        self.function = function
    }
}

public struct Function: Codable, Equatable, Sendable {
    public var description: String
    public var name: String
    public var parameters: [FunctionParameters]?

    public init(description: String, name: String, parameters: [FunctionParameters]? = nil) {
        self.description = description
        self.name = name
        self.parameters = parameters
    }
}

public struct FunctionParameters: Codable, Equatable, Sendable {
    public var type: String
    public var properties: [String: FunctionProperty]
    public var required: [String]?

    public init(type: String, properties: [String: FunctionProperty], required: [String]? = nil) {
        self.type = type
        self.properties = properties
        self.required = required
    }
}

public struct FunctionProperty: Codable, Equatable, Sendable {
    public var type: String
    public var description: String

    public init(type: String, description: String) {
        self.type = type
        self.description = description
    }
}

public enum ToolChoice: Codable, Equatable, Sendable {
    case unnamed(UnnamedToolChoice)
    case named(type: String, function: FunctionRef)

    private struct NamedToolChoice: Codable, Equatable, Sendable {
        var type: String
        var function: FunctionRef
    }

    public init(from decoder: Decoder) throws {
        let single = try decoder.singleValueContainer()
        if let unnamed = try? single.decode(UnnamedToolChoice.self) {
            self = .unnamed(unnamed)
            return
        }
        let named = try single.decode(NamedToolChoice.self)
        self = .named(type: named.type, function: named.function)
    }

    public func encode(to encoder: Encoder) throws {
        var single = encoder.singleValueContainer()
        switch self {
        case .unnamed(let unnamed):
            try single.encode(unnamed)
        case .named(let type, let function):
            try single.encode(NamedToolChoice(type: type, function: function))
        }
    }
}

public enum UnnamedToolChoice: String, Codable, Equatable, Sendable {
    case none
    case auto
    case required
}

public struct FunctionRef: Codable, Equatable, Sendable {
    public var name: String

    public init(name: String) {
        self.name = name
    }
}

public struct CompletionRequest: Codable, Equatable, Sendable {
    public var model: String
    public var messages: [Message]
    public var isStream: Bool?
    public var frequencyPenalty: Float?
    public var maxTokens: UInt32?
    public var presencePenalty: Float?
    public var responseFormat: ResponseFormat?
    public var stop: [String]?
    public var temperature: Float?
    public var topP: Float?
    public var tools: [Tool]?
    public var toolChoice: ToolChoice?
    public var logprobs: Bool?
    public var topLogprobs: UInt8?

    enum CodingKeys: String, CodingKey {
        case model
        case messages
        case isStream = "stream"
        case frequencyPenalty = "frequency_penalty"
        case maxTokens = "max_tokens"
        case presencePenalty = "presence_penalty"
        case responseFormat = "response_format"
        case stop
        case temperature
        case topP = "top_p"
        case tools
        case toolChoice = "tool_choice"
        case logprobs
        case topLogprobs = "top_logprobs"
    }

    public init(model: String, messages: [Message]) {
        self.model = model
        self.messages = messages
    }

    public func stream() -> Self {
        var copy = self
        copy.isStream = true
        return copy
    }

    public func frequencyPenalty(_ penalty: Float) -> Self {
        var copy = self
        copy.frequencyPenalty = penalty
        return copy
    }

    public func maxTokens(_ maxTokens: UInt32) -> Self {
        var copy = self
        copy.maxTokens = maxTokens
        return copy
    }

    public func presencePenalty(_ penalty: Float) -> Self {
        var copy = self
        copy.presencePenalty = penalty
        return copy
    }

    public func responseFormat(_ format: ResponseFormat) -> Self {
        var copy = self
        copy.responseFormat = format
        return copy
    }

    public func stop(_ stop: [String]) -> Self {
        var copy = self
        copy.stop = stop
        return copy
    }

    public func temperature(_ temperature: Float) -> Self {
        var copy = self
        copy.temperature = temperature
        return copy
    }

    public func topP(_ topP: Float) -> Self {
        var copy = self
        copy.topP = topP
        return copy
    }

    public func tools(_ tools: [Tool]) -> Self {
        var copy = self
        copy.tools = tools
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

    public func logprobs(_ enabled: Bool) -> Self {
        var copy = self
        copy.logprobs = enabled
        return copy
    }

    public func topLogprobs(_ value: UInt8) -> Self {
        var copy = self
        copy.topLogprobs = value
        return copy
    }
}

public extension Function {
    func asTool() -> Tool {
        Tool(function: self)
    }
}
