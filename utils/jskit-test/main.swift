import Foundation
import JSKit

private enum ReplCommand: String {
    case exit
    case quit
    case help
}

private func printHelp() {
    print("Commands:")
    print("  .help        Show this help")
    print("  .exit/.quit  Exit REPL")
    print("Any other input is evaluated as JavaScript.")
}

private func writeLine(_ text: String) {
    fputs("\(text)\n", stdout)
    fflush(stdout)
}

private func evaluateLoop() throws {
    let runtime = try JavaScriptRuntime()
    let context = try runtime.makeContext()

    writeLine("jskit-test REPL")
    writeLine("Type .help for commands.")

    while true {
        FileHandle.standardOutput.write(Data("> ".utf8))

        guard let line = readLine() else {
            writeLine("\nbye")
            return
        }

        let trimmed = line.trimmingCharacters(in: .whitespacesAndNewlines)
        if trimmed.isEmpty {
            continue
        }

        if trimmed.hasPrefix(".") {
            let commandText = String(trimmed.dropFirst())
            if let command = ReplCommand(rawValue: commandText) {
                switch command {
                case .help:
                    printHelp()
                    continue
                case .exit, .quit:
                    writeLine("bye")
                    return
                }
            }
        }

        do {
            let result = try context.evaluate(trimmed)
            let output = try result.string()
            writeLine(output)
        } catch {
            writeLine("error: \(error)")
        }
    }
}

do {
    try evaluateLoop()
} catch {
    fputs("failed to start repl: \(error)\n", stderr)
    exit(1)
}
