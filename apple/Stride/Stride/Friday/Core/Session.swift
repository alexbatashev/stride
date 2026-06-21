import Foundation

/// Holds the cloud server location and bearer token for the signed-in user and
/// mirrors them to `UserDefaults` so a session survives relaunches. The base URL
/// is kept after sign-out so the login screen can prefill it.
final class Session: @unchecked Sendable {
    static let shared = Session()

    private let lock = NSLock()
    private var storedBaseURL: URL?
    private var storedToken: String?

    private enum Key {
        static let baseURL = "friday.baseURL"
        static let token = "friday.token"
    }

    private init() {
        let defaults = UserDefaults.standard
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
        let defaults = UserDefaults.standard
        defaults.set(baseURL.absoluteString, forKey: Key.baseURL)
        defaults.set(token, forKey: Key.token)
    }

    func signOut() {
        lock.withLock { storedToken = nil }
        UserDefaults.standard.removeObject(forKey: Key.token)
    }
}
