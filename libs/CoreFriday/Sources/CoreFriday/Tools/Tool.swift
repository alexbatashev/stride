import Foundation
import LLMKit

public struct ToolArg {
    public var name: String
    public var value: String
}

public protocol Tool {
    func asLLM() -> LLMKit.Tool
    func id() -> String
    func execute(args: [ToolArg])
}
