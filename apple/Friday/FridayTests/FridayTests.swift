import Foundation
import SwiftData
import Testing
@testable import Friday

struct FridayTests {

    @Test("Persists conversation turns, attachments, and tool calls")
    func persistenceGraph() throws {
        let container = try ModelContainer(
            for: Conversation.self,
            ConversationTurn.self,
            TurnAttachment.self,
            ToolInvocation.self,
            configurations: ModelConfiguration(isStoredInMemoryOnly: true)
        )
        let context = ModelContext(container)

        let conversation = Conversation(title: "Graph Test")
        context.insert(conversation)
        let userSequence = conversation.nextSequenceNumber

        let userTurn = ConversationTurn(
            role: .user,
            text: "Summarize this file",
            sequenceNumber: userSequence,
            conversation: conversation
        )
        conversation.turns.append(userTurn)
        context.insert(userTurn)

        let attachment = TurnAttachment(
            kind: .file,
            fileName: "brief.md",
            mimeType: "text/markdown",
            localPath: "/tmp/brief.md",
            byteCount: 550,
            turn: userTurn
        )
        userTurn.attachments.append(attachment)
        context.insert(attachment)

        let assistantTurn = ConversationTurn(
            role: .assistant,
            text: "I parsed the file and prepared a summary.",
            sequenceNumber: userSequence + 1,
            modelIdentifier: "local.stub.v1",
            conversation: conversation
        )
        conversation.turns.append(assistantTurn)
        context.insert(assistantTurn)

        let tool = ToolInvocation(
            name: "parse_markdown",
            argumentsJSON: "{\"path\":\"/tmp/brief.md\"}",
            resultJSON: "{\"ok\":true}",
            status: .completed,
            endedAt: .now,
            turn: assistantTurn
        )
        assistantTurn.toolInvocations.append(tool)
        context.insert(tool)

        conversation.refreshPreview(using: "Summarize this file")
        try context.save()

        let fetched = try context.fetch(FetchDescriptor<Conversation>())
        #expect(fetched.count == 1)
        #expect(fetched[0].orderedTurns.count == 2)
        #expect(fetched[0].orderedTurns[0].role == .user)
        #expect(fetched[0].orderedTurns[0].attachments.count == 1)
        #expect(fetched[0].orderedTurns[1].role == .assistant)
        #expect(fetched[0].orderedTurns[1].toolInvocations.count == 1)
        #expect(fetched[0].previewText == "Summarize this file")
    }

    @Test("Orders turns by sequence number")
    func orderedTurnsUsesSequenceNumber() {
        let conversation = Conversation(title: "Ordering")

        let turn3 = ConversationTurn(role: .assistant, text: "three", sequenceNumber: 3, conversation: conversation)
        let turn1 = ConversationTurn(role: .user, text: "one", sequenceNumber: 1, conversation: conversation)
        let turn2 = ConversationTurn(role: .assistant, text: "two", sequenceNumber: 2, conversation: conversation)

        conversation.turns = [turn3, turn1, turn2]

        let ordered = conversation.orderedTurns.map(\.sequenceNumber)
        #expect(ordered == [1, 2, 3])
        #expect(conversation.nextSequenceNumber == 4)
    }

    @Test("Persists note blocks and attachments for rich note content")
    func notePersistenceGraph() throws {
        let container = try ModelContainer(
            for: Note.self,
            NoteBlock.self,
            NoteAttachment.self,
            configurations: ModelConfiguration(isStoredInMemoryOnly: true)
        )
        let context = ModelContext(container)

        let note = Note(title: "Prototype")
        context.insert(note)

        let textBlock = NoteBlock(
            kind: .text,
            orderIndex: note.nextOrderIndex,
            textContent: "Investigate architecture options.",
            note: note
        )
        note.blocks.append(textBlock)
        context.insert(textBlock)

        let tableBlock = NoteBlock(
            kind: .table,
            orderIndex: note.nextOrderIndex,
            payloadJSON: "{\"rows\":[[{\"text\":\"Owner\"},{\"text\":\"Status\"}],[{\"text\":\"Alex\"},{\"text\":\"In Progress\"}]]}",
            note: note
        )
        note.blocks.append(tableBlock)
        context.insert(tableBlock)

        let imageAttachment = NoteAttachment(
            kind: .image,
            fileName: "architecture.jpg",
            mimeType: "image/jpeg",
            localPath: "/tmp/architecture.jpg",
            byteCount: 22_500,
            block: tableBlock
        )
        tableBlock.attachments.append(imageAttachment)
        context.insert(imageAttachment)

        note.refreshPreview()
        try context.save()

        let fetched = try context.fetch(FetchDescriptor<Note>())
        #expect(fetched.count == 1)
        #expect(fetched[0].orderedBlocks.count == 2)
        #expect(fetched[0].orderedBlocks[0].kind == .text)
        #expect(fetched[0].orderedBlocks[1].kind == .table)
        #expect(fetched[0].orderedBlocks[1].attachments.count == 1)
        #expect(fetched[0].previewText == "Investigate architecture options.")
    }

    @Test("Orders note blocks by block order index")
    func orderedBlocksUsesOrderIndex() {
        let note = Note(title: "Ordering")

        let third = NoteBlock(kind: .text, orderIndex: 3, textContent: "Third", note: note)
        let first = NoteBlock(kind: .heading, orderIndex: 1, textContent: "First", note: note)
        let second = NoteBlock(kind: .checklist, orderIndex: 2, textContent: "- [ ] Second", note: note)

        note.blocks = [third, first, second]

        let ordered = note.orderedBlocks.map(\.orderIndex)
        #expect(ordered == [1, 2, 3])
        #expect(note.nextOrderIndex == 4)
    }
}
