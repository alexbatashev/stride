import Fluent
import Foundation

final class ProviderKeyModel: Model, @unchecked Sendable {
    static let schema = "provider_keys"

    @ID(key: .id)
    var id: UUID?

    @Parent(key: "user_id")
    var user: UserModel

    @Field(key: "name")
    var name: String

    /// "openai" | "anthropic" | "ollama"
    @Field(key: "provider")
    var provider: String

    @Field(key: "base_url")
    var baseURL: String

    @Field(key: "model_id")
    var modelId: String

    /// AES-256-GCM ciphertext (includes 16-byte authentication tag appended by CryptoKit)
    @Field(key: "encrypted_api_key")
    var encryptedApiKey: Data

    /// 12-byte AES-GCM nonce
    @Field(key: "api_key_nonce")
    var apiKeyNonce: Data

    @Timestamp(key: "created_at", on: .create)
    var createdAt: Date?

    @Timestamp(key: "updated_at", on: .update)
    var updatedAt: Date?

    init() {}

    init(
        id: UUID? = nil,
        userID: UUID,
        name: String,
        provider: String,
        baseURL: String,
        modelId: String,
        encryptedApiKey: Data,
        apiKeyNonce: Data
    ) {
        self.id = id
        self.$user.id = userID
        self.name = name
        self.provider = provider
        self.baseURL = baseURL
        self.modelId = modelId
        self.encryptedApiKey = encryptedApiKey
        self.apiKeyNonce = apiKeyNonce
    }
}
