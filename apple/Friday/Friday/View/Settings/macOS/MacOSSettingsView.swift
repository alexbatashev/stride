#if os(macOS)
    import SwiftUI

    public struct MacOSSettingsView: View {
        public init() {}

        public var body: some View {
            TabView {
                Tab("Models", systemImage: "cpu") {
                    ProvidersSettingsView()
                }
            }
            .frame(minWidth: 560, minHeight: 400)
        }
    }
#endif
