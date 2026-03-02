import Foundation
import JSKit
import LLMKit

public class JSTool: Tool {
    static let description: String = ""

    public func asLLM() -> LLMKit.Tool {
        fatalError()
    }

    public func id() -> String {
        "js_executable"
    }

    public func execute(args: [ToolArg]) {

    }

}
