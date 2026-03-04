import SwiftUI

public struct MainView: View {
    @Environment(ModelData.self) private var modelData
    @State private var isPresentingChatSettings = false

    public init() {}

    public var body: some View {
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
            ChatListView()
        } detail: {
            if let thread = modelData.selectedThread {
                ChatDetailView(thread: thread)
            } else {
                ContentUnavailableView(
                    "Select a Chat",
                    systemImage: "bubble.left.and.text.bubble.right",
                    description: Text("Choose or create a conversation from the middle column.")
                )
            }
        }
        .searchable(text: $modelData.searchString, prompt: "Search")
        .navigationSplitViewStyle(.balanced)
        .sheet(isPresented: $isPresentingChatSettings) {
            ChatSettingsView()
        }
    }

    private func sidebarIdentifier(for option: NavigationOptions) -> String {
        switch option {
        case .chat:
            return "navigationChatTab"
        }
    }
}
