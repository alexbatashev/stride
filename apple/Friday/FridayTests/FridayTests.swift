import Testing
import Foundation
@testable import Friday

@MainActor
struct FridayTests {

    @Test("Bootstraps with initial chat and note")
    func bootstrapState() {
        let modelData = ModelData()

        #expect(!modelData.conversations.isEmpty)
        #expect(!modelData.notes.isEmpty)
        #expect(modelData.selectedConversationID != nil)
        #expect(modelData.selectedNoteID != nil)
    }

    @Test("Can create and delete conversations")
    func conversationLifecycle() {
        let modelData = ModelData()
        let initialCount = modelData.conversations.count

        modelData.createConversation()
        #expect(modelData.conversations.count == initialCount + 1)

        modelData.deleteConversations(at: IndexSet(integer: 0))
        #expect(modelData.conversations.count >= 1)
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
