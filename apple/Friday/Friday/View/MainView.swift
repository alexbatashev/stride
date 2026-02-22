import SwiftUI

struct MainView: View {
    @State private var modelData = ModelData()
    @State private var isPresentingChatSettings = false

    var body: some View {
        NavigationSplitView {
            List {
                Section {
                    ForEach(NavigationOptions.mainPages) { page in
                        NavigationLink(value: page) {
                            Label(page.name, systemImage: page.symbolName)
                        }
                    }
                }
            }
            .navigationDestination(for: NavigationOptions.self) { page in
                NavigationStack(path: $modelData.path) {
                    page.contentViewForPage()
                }
//                .navigationDestination(for: Landmark.self) { landmark in
//                    LandmarkDetailView(landmark: landmark)
//                }
//                .navigationDestination(for: LandmarkCollection.self) { collection in
//                    CollectionDetailView(collection: collection)
//                }
//                .showsBadges()
            }
            .frame(minWidth: 150)
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
                ChatDetailView(modelData: modelData, conversationID: modelData.selectedConversationID)
            case .notes:
                NoteDetailView(modelData: modelData, noteID: modelData.selectedNoteID)
            }
        }
        .navigationSplitViewStyle(.balanced)
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    isPresentingChatSettings = true
                } label: {
                    Label("Chat Settings", systemImage: "slider.horizontal.3")
                }
            }
        }
        .sheet(isPresented: $isPresentingChatSettings) {
            ChatSettingsView(modelData: modelData)
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
