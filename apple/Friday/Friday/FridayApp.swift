import Friday
import SwiftUI

@main
struct FridayApp: App {
    @State private var modelData = ModelData()

    var body: some Scene {
        WindowGroup {
            MainView()
                .environment(modelData)
        }
        #if os(macOS)
            Settings {
                MacOSSettingsView()
                    .environment(modelData)
            }
        #endif
    }
}
