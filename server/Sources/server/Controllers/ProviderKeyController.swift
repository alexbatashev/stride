import Vapor
import Fluent
import Crypto
import LLMKit

// MARK: - Encryption helpers

/// Derives a 32-byte AES-256 key from the environment.
/// Reads ENCRYPTION_KEY (64 hex chars) or falls back to SHA-256(JWT_SECRET).
private func encryptionKey() throws -> SymmetricKey {
    if let hex = Environment.get("ENCRYPTION_KEY"), hex.count == 64,
       let bytes = Data(hexString: hex) {
        return SymmetricKey(data: bytes)
    }
    let secret = Environment.get("JWT_SECRET") ?? "dev-secret-change-in-prod"
    let digest = SHA256.hash(data: Data(secret.utf8))
    return SymmetricKey(data: digest)
}

private extension Data {
    init?(hexString: String) {
        guard hexString.count % 2 == 0 else { return nil }
        var bytes = [UInt8]()
        var index = hexString.startIndex
        while index < hexString.endIndex {
            let next = hexString.index(index, offsetBy: 2)
            guard let byte = UInt8(hexString[index..<next], radix: 16) else { return nil }
            bytes.append(byte)
            index = next
        }
        self = Data(bytes)
    }
}

private func encrypt(_ plaintext: String) throws -> (ciphertext: Data, nonce: Data) {
    let key = try encryptionKey()
    let nonce = AES.GCM.Nonce()
    let sealed = try AES.GCM.seal(Data(plaintext.utf8), using: key, nonce: nonce)
    // CryptoKit appends the 16-byte tag to ciphertext in combined form; use ciphertext + tag separately
    return (Data(sealed.ciphertext) + Data(sealed.tag), Data(nonce))
}

private func decrypt(ciphertext: Data, nonce nonceData: Data) throws -> String {
    let key = try encryptionKey()
    guard ciphertext.count >= 16 else {
        throw Abort(.internalServerError, reason: "Malformed encrypted key")
    }
    let tag = ciphertext.suffix(16)
    let ct = ciphertext.dropLast(16)
    let nonce = try AES.GCM.Nonce(data: nonceData)
    let box = try AES.GCM.SealedBox(nonce: nonce, ciphertext: ct, tag: tag)
    let plainData = try AES.GCM.open(box, using: key)
    guard let string = String(data: plainData, encoding: .utf8) else {
        throw Abort(.internalServerError, reason: "Decrypted key is not valid UTF-8")
    }
    return string
}

// MARK: - LLMKit factory

extension ProviderKeyModel {
    /// Decrypts the stored API key and returns the matching LLMKit `API` and token together.
    func toAPIAndToken() throws -> (API, String) {
        let token = try decrypt(ciphertext: encryptedApiKey, nonce: apiKeyNonce)
        let api: API
        switch provider {
        case "openai":    api = .openAI(OpenAI(baseURL: baseURL))
        case "anthropic": api = .anthropic(Anthropic(baseURL: baseURL))
        case "ollama":    api = .ollama(Ollama(baseURL: baseURL))
        default:
            throw Abort(.badRequest, reason: "Unknown provider '\(provider)'")
        }
        return (api, token)
    }
}

// MARK: - Request / Response types

struct CreateProviderKeyRequest: Content {
    let name: String
    let provider: String
    let baseURL: String
    let modelId: String
    let apiKey: String
}

struct UpdateProviderKeyRequest: Content {
    let name: String?
    let baseURL: String?
    let modelId: String?
    let apiKey: String?
}

struct ProviderKeyResponse: Content {
    let id: UUID
    let name: String
    let provider: String
    let baseURL: String
    let modelId: String
    let createdAt: Date?
    let updatedAt: Date?
}

private extension ProviderKeyModel {
    func toResponse() -> ProviderKeyResponse {
        ProviderKeyResponse(
            id: id!,
            name: name,
            provider: provider,
            baseURL: baseURL,
            modelId: modelId,
            createdAt: createdAt,
            updatedAt: updatedAt
        )
    }
}

// MARK: - Controller

struct ProviderKeyController: RouteCollection {
    private static let validProviders: Set<String> = ["openai", "anthropic", "ollama"]

    func boot(routes: any RoutesBuilder) throws {
        let jwtProtected = routes.grouped(UserPayload.authenticator(), UserPayload.guardMiddleware())
        let providerKeys = jwtProtected.grouped("api", "provider-keys")
        providerKeys.post(use: create)
        providerKeys.get(use: list)
        providerKeys.get(":id", use: get)
        providerKeys.patch(":id", use: update)
        providerKeys.delete(":id", use: delete)
    }

    @Sendable
    func create(req: Request) async throws -> ProviderKeyResponse {
        let payload = try req.auth.require(UserPayload.self)
        let body = try req.content.decode(CreateProviderKeyRequest.self)

        guard Self.validProviders.contains(body.provider) else {
            throw Abort(.badRequest, reason: "provider must be one of: openai, anthropic, ollama")
        }

        let (ciphertext, nonce) = try encrypt(body.apiKey)

        let record = ProviderKeyModel(
            userID: payload.userID,
            name: body.name,
            provider: body.provider,
            baseURL: body.baseURL,
            modelId: body.modelId,
            encryptedApiKey: ciphertext,
            apiKeyNonce: nonce
        )
        try await record.save(on: req.db)
        return record.toResponse()
    }

    @Sendable
    func list(req: Request) async throws -> [ProviderKeyResponse] {
        let payload = try req.auth.require(UserPayload.self)
        let records = try await ProviderKeyModel.query(on: req.db)
            .filter(\.$user.$id == payload.userID)
            .all()
        return records.map { $0.toResponse() }
    }

    @Sendable
    func get(req: Request) async throws -> ProviderKeyResponse {
        let payload = try req.auth.require(UserPayload.self)
        let record = try await requireOwned(req: req, userID: payload.userID)
        return record.toResponse()
    }

    @Sendable
    func update(req: Request) async throws -> ProviderKeyResponse {
        let payload = try req.auth.require(UserPayload.self)
        let body = try req.content.decode(UpdateProviderKeyRequest.self)
        let record = try await requireOwned(req: req, userID: payload.userID)

        if let name = body.name { record.name = name }
        if let baseURL = body.baseURL { record.baseURL = baseURL }
        if let modelId = body.modelId { record.modelId = modelId }
        if let apiKey = body.apiKey {
            let (ciphertext, nonce) = try encrypt(apiKey)
            record.encryptedApiKey = ciphertext
            record.apiKeyNonce = nonce
        }

        try await record.save(on: req.db)
        return record.toResponse()
    }

    @Sendable
    func delete(req: Request) async throws -> HTTPStatus {
        let payload = try req.auth.require(UserPayload.self)
        let record = try await requireOwned(req: req, userID: payload.userID)
        try await record.delete(on: req.db)
        return .noContent
    }

    // MARK: - Helpers

    private func requireOwned(req: Request, userID: UUID) async throws -> ProviderKeyModel {
        guard let id = req.parameters.get("id", as: UUID.self) else {
            throw Abort(.badRequest, reason: "Invalid provider key ID")
        }
        guard let record = try await ProviderKeyModel.query(on: req.db)
            .filter(\.$id == id)
            .filter(\.$user.$id == userID)
            .first()
        else {
            throw Abort(.notFound)
        }
        return record
    }
}
