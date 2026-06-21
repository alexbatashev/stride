import ComposableArchitecture
import SwiftUI

@main
struct StrideApp: App {
    @State private var store = Store(initialState: AppFeature.State()) {
        AppFeature()
    }

    var body: some Scene {
        WindowGroup {
            RootView(store: store)
                .tint(.accentColor)
        }
        #if os(macOS)
        .defaultSize(width: 1180, height: 800)
        #endif
    }
}
