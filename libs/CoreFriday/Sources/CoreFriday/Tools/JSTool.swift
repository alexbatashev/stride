import Foundation
import JSKit
import LLMKit

public final class JSTool: Tool, @unchecked Sendable {
    static let description: String = "Execute ECMAScript code and return concatenated console.log output."

    public init() {}

    public func asLLM() -> LLMKit.Tool {
        Function(
            description: Self.description,
            name: id(),
            parameters: [
                FunctionParameters(
                    type: "object",
                    properties: [
                        "code": FunctionProperty(
                            type: "string",
                            description: "Valid ECMAScript JavaScript source code to execute."
                        ),
                        "timeout": FunctionProperty(
                            type: "number",
                            description: "Execution timeout in seconds. Must be greater than 0 and no longer than 180."
                        ),
                    ],
                    required: ["code", "timeout"]
                )
            ]
        ).asTool()
    }

    public func id() -> String {
        "execute_js"
    }

    // Only accepts two arguments: "code" for the script code
    // and "timeout" for maximum timeout in seconds.
    public func execute(args: [ToolArg]) async -> String {
        let argByName = Dictionary(args.map { ($0.name, $0.value) }, uniquingKeysWith: { _, new in new })

        guard let code = argByName["code"], !code.isEmpty else {
            return "Error: Missing required argument 'code'."
        }
        guard let timeoutRaw = argByName["timeout"], let timeout = Int(timeoutRaw) else {
            return "Error: Missing or invalid required argument 'timeout'."
        }
        guard timeout > 0 else {
            return "Error: 'timeout' must be greater than 0 seconds."
        }
        guard timeout <= 180 else {
            return "Error: 'timeout' must not exceed 180 seconds."
        }

        do {
            let runtime = try JavaScriptRuntime()
            let context = try runtime.makeContext()
            _ = try context.evaluate(code, fileName: "<execute_js>", timeoutSeconds: timeout)
            return context.consumeConsoleOutput()
        } catch {
            return "Error: \(error)"
        }
    }

}
