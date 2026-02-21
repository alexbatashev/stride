import SwiftUI
import SwiftData

struct ConversationListView: View {
    @Bindable var modelData: ModelData

    @Environment(\.modelContext) private var modelContext
    @Query(sort: [SortDescriptor(\Conversation.updatedAt, order: .reverse)])
    private var conversations: [Conversation]

    var body: some View {
        List(selection: $modelData.selectedConversationID) {
            ForEach(conversations) { conversation in
                ConversationRow(conversation: conversation)
                    .tag(Optional(conversation.id))
            }
            .onDelete(perform: deleteConversations)
        }
        .accessibilityIdentifier("conversationList")
        .navigationTitle("Chats")
        .overlay {
            if conversations.isEmpty {
                ContentUnavailableView(
                    "No Chats",
                    systemImage: "bubble.left.and.text.bubble.right",
                    description: Text("Create a conversation to get started.")
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: createConversation) {
                    Label("New Chat", systemImage: "square.and.pencil")
                }
                .help("New Chat")
                .accessibilityIdentifier("newChatButton")
            }
        }
        .onAppear(perform: ensureInitialConversation)
    }

    private func ensureInitialConversation() {
        guard conversations.isEmpty else {
            if modelData.selectedConversationID == nil {
                modelData.selectedConversationID = conversations.first?.id
            }
            return
        }

        let conversation = Conversation(title: "Welcome")
        modelContext.insert(conversation)

        let turn = ConversationTurn(
            role: .assistant,
            text: "Welcome to Friday. Send a message to start a local, SwiftData-backed chat.",
            sequenceNumber: 0,
            modelIdentifier: "local.stub.v1",
            conversation: conversation
        )
        conversation.turns.append(turn)
        conversation.refreshPreview(using: turn.text)
        modelContext.insert(turn)

        do {
            try modelContext.save()
            modelData.selectedConversationID = conversation.id
        } catch {
            assertionFailure("Failed to seed initial conversation: \(error)")
        }
    }

    private func createConversation() {
        let conversation = Conversation()
        modelContext.insert(conversation)

        do {
            try modelContext.save()
            modelData.selectedConversationID = conversation.id
        } catch {
            assertionFailure("Failed to create conversation: \(error)")
        }
    }

    private func deleteConversations(at offsets: IndexSet) {
        for offset in offsets {
            let conversation = conversations[offset]
            for turn in conversation.turns {
                for attachment in turn.attachments {
                    modelContext.delete(attachment)
                }
                for tool in turn.toolInvocations {
                    modelContext.delete(tool)
                }
                modelContext.delete(turn)
            }
            modelContext.delete(conversation)
        }

        do {
            try modelContext.save()
            modelData.selectedConversationID = nil
        } catch {
            assertionFailure("Failed to delete conversation: \(error)")
        }
    }
}

private struct ConversationRow: View {
    let conversation: Conversation

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(conversation.title)
                .font(.headline)
                .lineLimit(1)

            if !conversation.previewText.isEmpty {
                Text(conversation.previewText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Text(conversation.updatedAt, style: .relative)
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 2)
    }
}
