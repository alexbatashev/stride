import Foundation

public struct RegisterRequest: Codable, Sendable {
    public var email: String
    public var password: String

    public init(email: String, password: String) {
        self.email = email
        self.password = password
    }
}

public struct LoginRequest: Codable, Sendable {
    public var email: String
    public var password: String

    public init(email: String, password: String) {
        self.email = email
        self.password = password
    }
}

public struct AuthResponse: Codable, Sendable {
    public var token: String
    public var id: UUID
    public var email: String

    public init(token: String, id: UUID, email: String) {
        self.token = token
        self.id = id
        self.email = email
    }
}
