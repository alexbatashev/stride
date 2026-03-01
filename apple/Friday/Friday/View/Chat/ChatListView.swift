import CoreFriday
import SwiftUI

struct ChatListView: View {
    @Environment(ModelData.self) private var modelData

    var body: some View {
        @Bindable var modelData = modelData

        let conversations = modelData.sortedConversations

        List(selection: $modelData.selectedConversation) {
            ForEach(conversations) { conversation in
                NavigationLink(value: conversation) {
                    ConversationRow(conversation: conversation)
                }
            }
            .onDelete(perform: modelData.deleteConversations)
        }
        .frame(idealWidth: 250)
        .navigationDestination(for: Conversation.self) { conversation in
            ChatDetailView(conversation: conversation)
        }
        .accessibilityIdentifier("chatList")
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
