import Vapor
import Fluent
import JWT
import FridayAPI

struct AuthController: RouteCollection {
    func boot(routes: any RoutesBuilder) throws {
        let auth = routes.grouped("api", "auth")
        auth.post("register", use: register)
        auth.post("login", use: login)
    }

    @Sendable
    func register(req: Request) async throws -> AuthResponse {
        let body = try req.content.decode(RegisterRequest.self)

        let existing = try await UserModel.query(on: req.db)
            .filter(\.$email == body.email)
            .first()

        guard existing == nil else {
            throw Abort(.conflict, reason: "Email already registered")
        }

        let hash = try await req.password.async.hash(body.password)
        let user = UserModel(email: body.email, passwordHash: hash)
        try await user.save(on: req.db)

        let payload = UserPayload(
            sub: .init(value: user.id!.uuidString),
            exp: .init(value: Date().addingTimeInterval(60 * 60 * 24 * 7)),
            userID: user.id!,
            email: user.email
        )
        let token = try req.jwt.sign(payload)

        return AuthResponse(token: token, id: user.id!, email: user.email)
    }

    @Sendable
    func login(req: Request) async throws -> AuthResponse {
        let body = try req.content.decode(LoginRequest.self)

        guard let user = try await UserModel.query(on: req.db)
            .filter(\.$email == body.email)
            .first()
        else {
            throw Abort(.unauthorized, reason: "Invalid credentials")
        }

        let valid = try await req.password.async.verify(body.password, created: user.passwordHash)
        guard valid else {
            throw Abort(.unauthorized, reason: "Invalid credentials")
        }

        let payload = UserPayload(
            sub: .init(value: user.id!.uuidString),
            exp: .init(value: Date().addingTimeInterval(60 * 60 * 24 * 7)),
            userID: user.id!,
            email: user.email
        )
        let token = try req.jwt.sign(payload)

        return AuthResponse(token: token, id: user.id!, email: user.email)
    }
}
