import Vapor
import Fluent
import Crypto

struct CreateAPIKeyRequest: Content {
    let name: String
}

struct APIKeyResponse: Content {
    let id: UUID
    let name: String
    let keyPrefix: String
    let createdAt: Date?
    let lastUsedAt: Date?
}

struct CreateAPIKeyResponse: Content {
    let id: UUID
    let name: String
    let keyPrefix: String
    let key: String
    let createdAt: Date?
}

struct APIKeyController: RouteCollection {
    func boot(routes: any RoutesBuilder) throws {
        let jwtProtected = routes.grouped(UserPayload.authenticator(), UserPayload.guardMiddleware())
        let keys = jwtProtected.grouped("api", "keys")
        keys.post(use: create)
        keys.get(use: list)
        keys.delete(":id", use: delete)
    }

    @Sendable
    func create(req: Request) async throws -> CreateAPIKeyResponse {
        let payload = try req.auth.require(UserPayload.self)
        let body = try req.content.decode(CreateAPIKeyRequest.self)

        let rawKey = "fri_" + UUID().uuidString.replacingOccurrences(of: "-", with: "")
                              + UUID().uuidString.replacingOccurrences(of: "-", with: "")
        let keyPrefix = String(rawKey.prefix(12))

        let tokenData = Data(rawKey.utf8)
        let hash = SHA256.hash(data: tokenData)
        let keyHash = hash.map { String(format: "%02x", $0) }.joined()

        let apiKey = APIKeyModel(userID: payload.userID, name: body.name, keyHash: keyHash, keyPrefix: keyPrefix)
        try await apiKey.save(on: req.db)

        return CreateAPIKeyResponse(
            id: apiKey.id!,
            name: apiKey.name,
            keyPrefix: apiKey.keyPrefix,
            key: rawKey,
            createdAt: apiKey.createdAt
        )
    }

    @Sendable
    func list(req: Request) async throws -> [APIKeyResponse] {
        let payload = try req.auth.require(UserPayload.self)

        let keys = try await APIKeyModel.query(on: req.db)
            .filter(\.$user.$id == payload.userID)
            .all()

        return keys.map {
            APIKeyResponse(
                id: $0.id!,
                name: $0.name,
                keyPrefix: $0.keyPrefix,
                createdAt: $0.createdAt,
                lastUsedAt: $0.lastUsedAt
            )
        }
    }

    @Sendable
    func delete(req: Request) async throws -> HTTPStatus {
        let payload = try req.auth.require(UserPayload.self)

        guard let keyID = req.parameters.get("id", as: UUID.self) else {
            throw Abort(.badRequest, reason: "Invalid key ID")
        }

        guard let apiKey = try await APIKeyModel.query(on: req.db)
            .filter(\.$id == keyID)
            .filter(\.$user.$id == payload.userID)
            .first()
        else {
            throw Abort(.notFound)
        }

        try await apiKey.delete(on: req.db)
        return .noContent
    }
}
