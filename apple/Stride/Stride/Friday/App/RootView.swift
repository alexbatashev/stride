import ComposableArchitecture
import SwiftUI

struct RootView: View {
    @Bindable var store: StoreOf<AppFeature>

    var body: some View {
        Group {
            if let homeStore = store.scope(state: \.home, action: \.home) {
                HomeView(store: homeStore)
                    .transition(.opacity)
            } else {
                AuthView(store: store.scope(state: \.auth, action: \.auth))
                    .transition(.opacity.combined(with: .scale(scale: 0.98)))
            }
        }
        .animation(.smooth(duration: 0.35), value: store.home == nil)
    }
}
