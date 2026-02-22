import SwiftUI

struct MainView: View {
    @State private var modelData = ModelData()

    var body: some View {
        NavigationSplitView {
            List(NavigationOptions.mainPages, selection: $modelData.selectedNavigation) { option in
                Label(option.name, systemImage: option.symbolName)
                    .accessibilityIdentifier(sidebarIdentifier(for: option))
                    .contentShape(Rectangle())
                    .onTapGesture {
                        modelData.selectedNavigation = option
                    }
                    .tag(Optional(option))
            }
            .navigationTitle("Friday")
            .listStyle(.sidebar)
        } content: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                ConversationListView(modelData: modelData)
            case .notes:
                NotesListView(modelData: modelData)
            }
        } detail: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                ChatView(conversationID: modelData.selectedConversationID)
            case .notes:
                NoteDetailView(noteID: modelData.selectedNoteID)
            }
        }
        .navigationSplitViewStyle(.balanced)
    }

    private func sidebarIdentifier(for option: NavigationOptions) -> String {
        switch option {
        case .chat:
            return "navigationChatTab"
        case .notes:
            return "navigationNotesTab"
        }
    }
}
