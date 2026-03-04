import Testing
import Foundation
@testable import Friday

@MainActor
struct FridayTests {

    @Test("Bootstraps with initial chat and note")
    func bootstrapState() {
        let modelData = ModelData()

        #expect(!modelData.threads.isEmpty)
        #expect(!modelData.notes.isEmpty)
        #expect(modelData.selectedThread != nil)
        #expect(modelData.selectedNote != nil)
    }

    @Test("Can create and delete threads")
    func threadLifecycle() {
        let modelData = ModelData()
        let initialCount = modelData.threads.count

        modelData.createThread()
        #expect(modelData.threads.count == initialCount + 1)

        modelData.deleteThreads(at: IndexSet(integer: 0))
        #expect(modelData.threads.count >= 1)
    }

    @Test("Can create and delete notes")
    func noteLifecycle() {
        let modelData = ModelData()
        let initialCount = modelData.notes.count

        modelData.createNote()
        #expect(modelData.notes.count == initialCount + 1)

        modelData.deleteNotes(at: IndexSet(integer: 0))
        #expect(modelData.notes.count >= 1)
    }
}
