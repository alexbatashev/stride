import SwiftUI

struct MainView: View {
    @Environment(ModelData.self) private var modelData
    @State private var isPresentingChatSettings = false

    var body: some View {
        @Bindable var modelData = modelData

        NavigationSplitView {
            List(selection: $modelData.selectedNavigation) {
                Section {
                    ForEach(NavigationOptions.mainPages) { page in
                        NavigationLink(value: page) {
                            Label(page.name, systemImage: page.symbolName)
                                .accessibilityIdentifier(sidebarIdentifier(for: page))
                        }
                    }
                }
            }
            .listStyle(.sidebar)
        } content: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                ChatListView()
            case .notes:
                NotesListView()
            }
        } detail: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                if let thread = modelData.selectedThread {
                    ChatDetailView(thread: thread)
                } else {
                    ContentUnavailableView(
                        "Select a Chat",
                        systemImage: "bubble.left.and.text.bubble.right",
                        description: Text("Choose or create a conversation from the middle column.")
                    )
                }
            case .notes:
                if let note = modelData.selectedNote {
                    NoteDetailView(note: note)
                } else {
                    ContentUnavailableView(
                        "Select a Note",
                        systemImage: "note.text",
                        description: Text("Choose or create a note from the middle column.")
                    )
                }
            }
        }
        .searchable(text: $modelData.searchString, prompt: "Search")
        .navigationSplitViewStyle(.balanced)
//        .toolbar {
//            ToolbarItem(placement: .automatic) {
//                Button {
//                    isPresentingChatSettings = true
//                } label: {
//                    Label("Chat Settings", systemImage: "slider.horizontal.3")
//                }
//            }
//        }
        .sheet(isPresented: $isPresentingChatSettings) {
            ChatSettingsView()
        }
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
