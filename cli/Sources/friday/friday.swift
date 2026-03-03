import CoreFriday
import Foundation

@main
struct friday {
    static func main() async {
        var provider = ChatProviderConfiguration.starterOllama()
        let providerId = provider.id.uuidString

        let transport = DirectChatTransport(provider: provider)
        let jsTool = JSTool()
        let chat = ChatService(transports: [transport])
        var toolsEnabled = false

        let model: String
        let models = await chat.listModels()
        guard let firstModel = models.first?.model else {
            writeStderr(
                "No Ollama models found at \(provider.baseURL). Pull one first (for example: `ollama pull llama3.2`).\n",
            )
            return
        }
        model = firstModel
        provider.defaultModel = firstModel
        await chat.setModel(providerId: providerId, modelId: model)

        print("Friday CLI")
        print("Provider: \(provider.name) (\(provider.baseURL))")
        print("Model: \(model)")
        print("Type your prompt and press Enter. Type `exit` to quit.")
        print("Type `/tools on` or `/tools off` to toggle tool execution (default: off).")
        let threadId = UUID()

        while true {
            writeStdout("> ")

            guard let line = readLine() else {
                print("")
                break
            }

            let prompt = line.trimmingCharacters(in: .whitespacesAndNewlines)
            if prompt.isEmpty {
                continue
            }

            if prompt.lowercased() == "exit" {
                break
            }
            if prompt.lowercased() == "/tools on" {
                toolsEnabled = true
                print("Tools enabled.")
                continue
            }
            if prompt.lowercased() == "/tools off" {
                toolsEnabled = false
                print("Tools disabled.")
                continue
            }

            let userTurn = ChatMessage(
                id: UUID(),
                threadId: threadId,
                userId: nil,
                parentId: nil,
                providerId: providerId,
                modelId: model,
                modelName: model,
                role: .user,
                thinking: nil,
                content: prompt
            )
            var printedCount = 0
            var printedThinkingCount = 0
            var thinkingStarted = false
            var thinkingLineOpen = false
            var activeMessageID: UUID?
            writeStdout("friday> ")

            do {
                let tools: [any Tool] = toolsEnabled ? [jsTool] : []
                let stream = await chat.addMessage(tools: tools, next: userTurn)
                for try await partial in stream {
                    if activeMessageID != partial.id {
                        activeMessageID = partial.id
                        printedCount = 0
                        printedThinkingCount = 0
                        if thinkingLineOpen {
                            writeStderr("\n")
                            thinkingLineOpen = false
                        }
                        thinkingStarted = false
                    }

                    let fullThinking = partial.thinking ?? ""
                    let thinkingSuffix = String(fullThinking.dropFirst(printedThinkingCount))
                    if !thinkingSuffix.isEmpty {
                        if !thinkingStarted {
                            writeStderr("thinking> ")
                            thinkingStarted = true
                            thinkingLineOpen = true
                        }
                        writeStderr(thinkingSuffix)
                        printedThinkingCount = fullThinking.count
                    }

                    let fullText = partial.content
                    let suffix = String(fullText.dropFirst(printedCount))
                    if !suffix.isEmpty {
                        if thinkingLineOpen {
                            writeStderr("\n")
                            thinkingLineOpen = false
                        }
                        writeStdout(suffix)
                        printedCount = fullText.count
                    }
                }
                if thinkingLineOpen {
                    writeStderr("\n")
                }
                print("")
            } catch {
                if thinkingLineOpen {
                    writeStderr("\n")
                }
                print("")
                let message = "Request failed: \(error)"
                writeStderr("\(message)\n")
            }
        }
    }

    private static func writeStdout(_ text: String) {
        FileHandle.standardOutput.write(Data(text.utf8))
    }

    private static func writeStderr(_ text: String) {
        FileHandle.standardError.write(Data(text.utf8))
    }
}
