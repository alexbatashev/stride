import Foundation
import LLMKit

public struct ToolArg: Sendable {
    public var name: String
    public var value: String
}

public protocol Tool: Sendable {
    func asLLM() -> LLMKit.Tool
    func id() -> String
    func execute(args: [ToolArg]) async -> String
}
