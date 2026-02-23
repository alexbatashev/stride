import Vapor
import Fluent
import Crypto

struct APIKeyUser: Authenticatable {
    let userID: UUID
    let keyID: UUID
}

struct APIKeyAuthenticator: AsyncBearerAuthenticator {
    func authenticate(bearer: BearerAuthorization, for req: Request) async throws {
        let tokenData = Data(bearer.token.utf8)
        let hash = SHA256.hash(data: tokenData)
        let keyHash = hash.map { String(format: "%02x", $0) }.joined()

        guard let apiKey = try await APIKeyModel.query(on: req.db)
            .filter(\.$keyHash == keyHash)
            .first()
        else {
            return
        }

        try await APIKeyModel.query(on: req.db)
            .filter(\.$id == apiKey.id!)
            .set(\.$lastUsedAt, to: Date())
            .update()

        req.auth.login(APIKeyUser(userID: apiKey.$user.id, keyID: apiKey.id!))
    }
}
