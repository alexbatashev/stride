import Foundation
import Observation

@Observable
@MainActor
final class ModelData {
    var selectedNavigation: NavigationOptions?
    var selectedConversationID: UUID?
    var selectedNoteID: UUID?

    init() {
        if ProcessInfo.processInfo.arguments.contains("-ui-testing-open-notes") {
            selectedNavigation = .notes
        } else {
            selectedNavigation = .chat
        }
    }
}
