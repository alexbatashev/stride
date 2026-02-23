import JWT
import Vapor

struct UserPayload: JWTPayload, Authenticatable {
    var sub: SubjectClaim
    var exp: ExpirationClaim
    var userID: UUID
    var email: String

    func verify(using signer: JWTSigner) throws {
        try exp.verifyNotExpired()
    }
}
