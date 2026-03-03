import Foundation

public struct Message: Codable, Equatable, Sendable {
    public var role: Role
    public var content: String
    public var thinking: String?

    public init(role: Role, content: String, thinking: String? = nil) {
        self.role = role
        self.content = content
        self.thinking = thinking
    }
}

public enum Role: String, Codable, Equatable, Sendable {
    case system
    case assistant
    case user
    case tool
}

public struct Completion: Codable, Equatable, Sendable {
    public var id: String
    public var created: Int
    public var model: String
    public var choices: [CompletionChoice]
    public var usage: Usage

    public init(id: String, created: Int, model: String, choices: [CompletionChoice], usage: Usage) {
        self.id = id
        self.created = created
        self.model = model
        self.choices = choices
        self.usage = usage
    }
}

public struct StreamResponseChunk: Codable, Equatable, Sendable {
    public var id: String
    public var object: String
    public var created: Int
    public var model: String
    public var systemFingerprint: String?
    public var choices: [CompletionChoice]

    enum CodingKeys: String, CodingKey {
        case id
        case object
        case created
        case model
        case systemFingerprint = "system_fingerprint"
        case choices
    }

    public init(
        id: String,
        object: String,
        created: Int,
        model: String,
        systemFingerprint: String?,
        choices: [CompletionChoice]
    ) {
        self.id = id
        self.object = object
        self.created = created
        self.model = model
        self.systemFingerprint = systemFingerprint
        self.choices = choices
    }
}

public struct Delta: Codable, Equatable, Sendable {
    public var content: String?

    public init(content: String?) {
        self.content = content
    }
}

public struct CompletionChoice: Codable, Equatable, Sendable {
    public var message: Message?
    public var text: String?
    public var index: UInt16
    public var delta: Delta?
    public var logprobs: UInt16?
    public var finishReason: String?

    enum CodingKeys: String, CodingKey {
        case message
        case text
        case index
        case delta
        case logprobs
        case finishReason = "finish_reason"
    }

    public init(
        message: Message?,
        text: String?,
        index: UInt16,
        delta: Delta?,
        logprobs: UInt16?,
        finishReason: String?
    ) {
        self.message = message
        self.text = text
        self.index = index
        self.delta = delta
        self.logprobs = logprobs
        self.finishReason = finishReason
    }
}

public struct Usage: Codable, Equatable, Sendable {
    public var promptTokens: UInt32
    public var completionTokens: UInt32
    public var totalTokens: UInt32

    enum CodingKeys: String, CodingKey {
        case promptTokens = "prompt_tokens"
        case completionTokens = "completion_tokens"
        case totalTokens = "total_tokens"
    }

    public init(promptTokens: UInt32, completionTokens: UInt32, totalTokens: UInt32) {
        self.promptTokens = promptTokens
        self.completionTokens = completionTokens
        self.totalTokens = totalTokens
    }
}

public struct ModelDesc: Codable, Equatable, Sendable {
    public var id: String
    public var object: String
    public var created: UInt64?
    public var ownedBy: String?

    enum CodingKeys: String, CodingKey {
        case id
        case object
        case created
        case ownedBy = "owned_by"
    }

    public init(id: String, object: String, created: UInt64?, ownedBy: String?) {
        self.id = id
        self.object = object
        self.created = created
        self.ownedBy = ownedBy
    }
}

public struct EmbeddingData: Codable, Equatable, Sendable {
    public var object: String
    public var index: UInt32
    public var embedding: [Float]

    public init(object: String, index: UInt32, embedding: [Float]) {
        self.object = object
        self.index = index
        self.embedding = embedding
    }
}

public struct EmbeddingResponse: Codable, Equatable, Sendable {
    public var object: String
    public var model: String
    public var data: EmbeddingData
    public var usage: Usage

    enum CodingKeys: String, CodingKey {
        case object
        case model
        case data
        case usage
    }

    public init(object: String, model: String, data: EmbeddingData, usage: Usage) {
        self.object = object
        self.model = model
        self.data = data
        self.usage = usage
    }

    public init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        object = try container.decode(String.self, forKey: .object)
        model = try container.decode(String.self, forKey: .model)
        usage = try container.decode(Usage.self, forKey: .usage)

        if let single = try? container.decode(EmbeddingData.self, forKey: .data) {
            data = single
            return
        }

        let list = try container.decode([EmbeddingData].self, forKey: .data)
        guard let first = list.first else {
            throw DecodingError.dataCorruptedError(forKey: .data, in: container, debugDescription: "Expected at least one embedding")
        }
        data = first
    }
}
