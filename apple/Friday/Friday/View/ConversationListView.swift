import CoreFriday
import SwiftUI

struct ConversationListView: View {
    @Bindable var modelData: ModelData

    var body: some View {
        let conversations = modelData.sortedConversations

        List(selection: $modelData.selectedConversationID) {
            ForEach(conversations) { conversation in
                ConversationRow(conversation: conversation)
                    .tag(Optional(conversation.id))
            }
            .onDelete(perform: modelData.deleteConversations)
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
                Button(action: modelData.createConversation) {
                    Label("New Chat", systemImage: "square.and.pencil")
                }
                .help("New Chat")
                .accessibilityIdentifier("newChatButton")
            }
        }
        .onAppear(perform: modelData.ensureInitialConversation)
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
