import CoreFriday
import SwiftUI

struct ChatListView: View {
    @Environment(ModelData.self) private var modelData

    var body: some View {
        @Bindable var modelData = modelData

        let threads = modelData.sortedThreads

        List(selection: $modelData.selectedThread) {
            ForEach(threads) { thread in
                NavigationLink(value: thread) {
                    ThreadRow(thread: thread)
                }
            }
            .onDelete(perform: modelData.deleteThreads)
        }
        .frame(idealWidth: 250)
        .navigationDestination(for: ChatThread.self) { thread in
            ChatDetailView(thread: thread)
        }
        .accessibilityIdentifier("chatList")
        .overlay {
            if threads.isEmpty {
                ContentUnavailableView(
                    "No Chats",
                    systemImage: "bubble.left.and.text.bubble.right",
                    description: Text("Create a conversation to get started.")
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: modelData.createThread) {
                    Label("New Chat", systemImage: "square.and.pencil")
                }
                .help("New Chat")
                .accessibilityIdentifier("newChatButton")
            }
        }
        .task { await modelData.loadThreads() }
    }
}

private struct ThreadRow: View {
    let thread: ChatThread

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(thread.title.isEmpty ? "New Chat" : thread.title)
                .font(.headline)
                .lineLimit(1)

            if !thread.previewText.isEmpty {
                Text(thread.previewText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Text(thread.updatedAt, style: .relative)
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 2)
    }
}
