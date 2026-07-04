import ComposableArchitecture
import SwiftUI

@main
struct StrideApp: App {
    @State private var store = Store(initialState: AppFeature.State()) {
        AppFeature()
    }

    @SceneBuilder
    var body: some Scene {
        mainWindow

        #if os(macOS)
        MenuBarExtra("Stride Operator", systemImage: "sparkles") {
            OperatorMenu(store: store)
        }
        #endif
    }

    private var mainWindow: some Scene {
        WindowGroup("Stride", id: "main") {
            RootView(store: store)
                .tint(.accentColor)
        }
        #if os(macOS)
        .defaultSize(width: 1180, height: 800)
        #endif
    }
}

#if os(macOS)
private struct OperatorMenu: View {
    let store: StoreOf<AppFeature>
    @Environment(\.openWindow) private var openWindow

    var body: some View {
        Button("Open Stride") {
            openWindow(id: "main")
        }

        Button("New Local Thread") {
            openWindow(id: "main")
            store.send(.home(.sidebarSelected(.local)))
            store.send(.home(.newThreadTapped))
        }
        .disabled(store.home == nil)

        Divider()

        Button("Quit") {
            NSApplication.shared.terminate(nil)
        }
    }
}
#endif
