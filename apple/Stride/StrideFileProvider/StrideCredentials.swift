import Foundation

/// Reads the signed-in session (server URL + bearer token) from the App Group
/// shared with the host app. The extension runs in its own process, so this is
/// the only way it learns who's logged in.
struct StrideCredentials {
    let baseURL: URL
    let token: String

    static let appGroup = "group.me.batashev.Stride"

    private enum Key {
        static let baseURL = "stride.baseURL"
        static let token = "stride.token"
    }

    static func current() -> StrideCredentials? {
        guard let defaults = UserDefaults(suiteName: appGroup),
              let raw = defaults.string(forKey: Key.baseURL),
              let url = URL(string: raw),
              let token = defaults.string(forKey: Key.token), !token.isEmpty
        else { return nil }
        return StrideCredentials(baseURL: url, token: token)
    }
}
