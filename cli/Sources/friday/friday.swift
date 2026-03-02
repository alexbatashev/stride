import CoreFriday
import Foundation

@main
struct friday {
    static func main() async {
        var provider = ChatProviderConfiguration.starterOllama()
        let providerId = provider.id.uuidString

        let transport = DirectChatTransport(provider: provider)
        let chat = ChatStream(transports: [transport])

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

            let userTurn = ConversationTurn(
                role: .user,
                text: prompt,
                createdAt: .now
            )
            var printedCount = 0
            writeStdout("friday> ")

            do {
                let stream = await chat.addMessage(tools: [], next: userTurn)
                for try await partial in stream {
                    let fullText = partial.text
                    let suffix = String(fullText.dropFirst(printedCount))
                    if !suffix.isEmpty {
                        writeStdout(suffix)
                        printedCount = fullText.count
                    }
                }
                print("")
            } catch {
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
