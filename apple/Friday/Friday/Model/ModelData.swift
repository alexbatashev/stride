import Foundation
import Observation

@Observable
@MainActor
final class ModelData {
    var selectedNavigation: NavigationOptions? = .chat
    var selectedConversationID: UUID?
}
