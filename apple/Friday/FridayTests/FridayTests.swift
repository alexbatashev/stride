import Testing
import Foundation
@testable import Friday

@MainActor
struct FridayTests {

    @Test("Bootstraps with initial chat thread")
    func bootstrapState() async {
        let modelData = ModelData()
        await modelData.loadThreads()

        #expect(!modelData.threads.isEmpty)
        #expect(modelData.selectedThread != nil)
    }

    @Test("Can create and delete threads")
    func threadLifecycle() async {
        let modelData = ModelData()
        await modelData.loadThreads()
        let initialCount = modelData.threads.count

        modelData.createThread()
        #expect(modelData.threads.count == initialCount + 1)

        modelData.deleteThreads(at: IndexSet(integer: 0))
        #expect(modelData.threads.count >= 1)
    }
}
