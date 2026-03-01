import CQuickJS

public enum JSError: Error, Sendable {
    case runtimeCreationFailed
    case contextCreationFailed
    case evaluationFailed(String)
    case stringConversionFailed
}

public final class JavaScriptRuntime {
    let rawRuntime: UnsafeMutableRawPointer

    public init() throws {
        guard let runtime = qjs_runtime_new() else {
            throw JSError.runtimeCreationFailed
        }
        self.rawRuntime = runtime
    }

    deinit {
        qjs_runtime_free(rawRuntime)
    }

    public func makeContext() throws -> JavaScriptContext {
        guard let context = qjs_context_new(rawRuntime) else {
            throw JSError.contextCreationFailed
        }
        return JavaScriptContext(runtime: self, rawContext: context)
    }
}

public final class JavaScriptContext {
    private let runtime: JavaScriptRuntime
    let rawContext: UnsafeMutableRawPointer

    init(runtime: JavaScriptRuntime, rawContext: UnsafeMutableRawPointer) {
        self.runtime = runtime
        self.rawContext = rawContext
    }

    deinit {
        qjs_context_free(rawContext)
    }

    @discardableResult
    public func evaluate(
        _ source: String,
        fileName: String = "<eval>",
        flags: Int32 = 0
    ) throws -> JavaScriptValue {
        let result = source.withCString { sourceCString in
            fileName.withCString { fileNameCString in
                qjs_context_eval(rawContext, sourceCString, fileNameCString, flags)
            }
        }

        guard let result else {
            throw JSError.contextCreationFailed
        }

        if qjs_value_is_exception(rawContext, result) != 0 {
            throw JSError.evaluationFailed(consumeExceptionMessage())
        }

        return JavaScriptValue(context: self, rawValue: result)
    }

    fileprivate func consumeExceptionMessage() -> String {
        guard let cString = qjs_context_exception_to_string(rawContext) else {
            return "Unknown QuickJS exception"
        }
        defer { qjs_cstring_free(cString) }

        return String(cString: cString)
    }
}

public final class JavaScriptValue {
    private let context: JavaScriptContext
    private let rawValue: UnsafeMutableRawPointer

    init(context: JavaScriptContext, rawValue: UnsafeMutableRawPointer) {
        self.context = context
        self.rawValue = rawValue
    }

    deinit {
        qjs_value_free(context.rawContext, rawValue)
    }

    public func string() throws -> String {
        guard let cString = qjs_value_to_string(context.rawContext, rawValue) else {
            throw JSError.stringConversionFailed
        }
        defer { qjs_cstring_free(cString) }

        return String(cString: cString)
    }
}
