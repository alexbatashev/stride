import Foundation

/// Holds the cloud server location and bearer token for the signed-in user and
/// mirrors them to a shared App Group `UserDefaults` so a session survives
/// relaunches *and* is readable by the File Provider extension (a separate
/// process). The base URL is kept after sign-out so the login screen can prefill
/// it.
final class Session: @unchecked Sendable {
    static let shared = Session()

    /// App Group shared with the File Provider extension.
    static let appGroup = "group.me.batashev.Stride"

    private let lock = NSLock()
    private var storedBaseURL: URL?
    private var storedToken: String?

    enum Key {
        static let baseURL = "friday.baseURL"
        static let token = "friday.token"
    }

    /// Shared store when the App Group is available, otherwise the standard one.
    private let defaults: UserDefaults

    private init() {
        if let suite = UserDefaults(suiteName: Session.appGroup) {
            Session.migrate(from: .standard, to: suite)
            defaults = suite
        } else {
            defaults = .standard
        }
        if let raw = defaults.string(forKey: Key.baseURL) {
            storedBaseURL = URL(string: raw)
        }
        storedToken = defaults.string(forKey: Key.token)
    }

    var baseURL: URL? {
        lock.withLock { storedBaseURL }
    }

    var token: String? {
        lock.withLock { storedToken }
    }

    var isAuthenticated: Bool {
        lock.withLock { storedBaseURL != nil && storedToken != nil }
    }

    func signIn(baseURL: URL, token: String) {
        lock.withLock {
            storedBaseURL = baseURL
            storedToken = token
        }
        defaults.set(baseURL.absoluteString, forKey: Key.baseURL)
        defaults.set(token, forKey: Key.token)
        FridayProviderBridge.shared.register()
    }

    func signOut() {
        lock.withLock { storedToken = nil }
        defaults.removeObject(forKey: Key.token)
        FridayProviderBridge.shared.unregister()
    }

    /// Copies an existing standard-defaults session into the App Group store on
    /// first launch after the extension was added.
    private static func migrate(from standard: UserDefaults, to shared: UserDefaults) {
        if shared.string(forKey: Key.baseURL) == nil, let raw = standard.string(forKey: Key.baseURL) {
            shared.set(raw, forKey: Key.baseURL)
        }
        if shared.string(forKey: Key.token) == nil, let token = standard.string(forKey: Key.token) {
            shared.set(token, forKey: Key.token)
        }
    }
}
