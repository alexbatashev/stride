import SwiftUI

/// Navigation options shown in the left sidebar.
enum NavigationOptions: Equatable, Hashable, Identifiable {
    case chat
    case notes

    static let mainPages: [NavigationOptions] = [.chat, .notes]

    var id: String {
        switch self {
        case .chat: return "chat"
        case .notes: return "notes"
        }
    }

    var name: LocalizedStringResource {
        switch self {
        case .chat: return "Chat"
        case .notes: return "Notes"
        }
    }

    var symbolName: String {
        switch self {
        case .chat: return "bubble.left.and.bubble.right"
        case .notes: return "note.text"
        }
    }
}
