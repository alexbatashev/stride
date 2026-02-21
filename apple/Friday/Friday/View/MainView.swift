import SwiftUI

struct MainView: View {
    @State private var modelData = ModelData()

    var body: some View {
        NavigationSplitView {
            List(NavigationOptions.mainPages, selection: $modelData.selectedNavigation) { option in
                Label(option.name, systemImage: option.symbolName)
                    .tag(Optional(option))
            }
            .navigationTitle("Friday")
            .listStyle(.sidebar)
        } content: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                ConversationListView(modelData: modelData)
            }
        } detail: {
            switch modelData.selectedNavigation ?? .chat {
            case .chat:
                ChatView(conversationID: modelData.selectedConversationID)
            }
        }
        .navigationSplitViewStyle(.balanced)
    }
}
