import SwiftUI
#if canImport(UIKit)
import UIKit
#elseif canImport(AppKit)
import AppKit
#endif

/// Shared spacing, radii and widths so the UI stays visually consistent.
enum Metrics {
    static let bubbleRadius: CGFloat = 20
    static let composerRadius: CGFloat = 24
    static let cardRadius: CGFloat = 16
    static let gutter: CGFloat = 16
    static let tight: CGFloat = 8
    static let messageSpacing: CGFloat = 18
    static let maxBubbleWidth: CGFloat = 560
    static let maxReadingWidth: CGFloat = 720
}

extension Color {
    /// Tint behind the user's own messages.
    static let userBubble = Color.accentColor

    /// Subtle fill used for tool/system chips and code blocks.
    static let subtleFill = Color.primary.opacity(0.06)
    static let hairline = Color.primary.opacity(0.08)
}

/// Copies text to the system pasteboard on either platform.
enum Clipboard {
    static func copy(_ string: String) {
        #if os(iOS) || os(visionOS)
        UIPasteboard.general.string = string
        #elseif os(macOS)
        NSPasteboard.general.clearContents()
        NSPasteboard.general.setString(string, forType: .string)
        #endif
    }
}

/// Light haptic feedback on touch devices; a no-op elsewhere.
enum Haptics {
    static func tap() {
        #if os(iOS)
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
        #endif
    }
}
