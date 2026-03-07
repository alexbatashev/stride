import SwiftUI

/// Navigation options shown in the left sidebar.
enum NavigationOptions: Equatable, Hashable, Identifiable {
    case chat

    static let mainPages: [NavigationOptions] = [.chat]

    var id: String {
        switch self {
        case .chat: return "chat"
        }
    }

    var name: LocalizedStringResource {
        switch self {
        case .chat: return "Chat"
        }
    }

    var symbolName: String {
        switch self {
        case .chat: return "bubble.left.and.bubble.right"
        }
    }
}
