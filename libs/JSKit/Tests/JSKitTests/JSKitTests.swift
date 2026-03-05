import Testing
@testable import JSKit

@Test func evaluatesSimpleExpression() throws {
    let runtime = try JavaScriptRuntime()
    let context = try runtime.makeContext()
    let value = try context.evaluate("Math.sqrt(9)")

    #expect(try value.string() == "3")
}

@Test func consoleLogIsAvailable() throws {
    let runtime = try JavaScriptRuntime()
    let context = try runtime.makeContext()
    let value = try context.evaluate("console.log('hello from test')")

    #expect(try value.string() == "undefined")
}
