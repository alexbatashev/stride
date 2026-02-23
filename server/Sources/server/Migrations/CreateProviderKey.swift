import Fluent

struct CreateProviderKey: AsyncMigration {
    func prepare(on database: any Database) async throws {
        try await database.schema("provider_keys")
            .id()
            .field("user_id", .uuid, .required, .references("users", "id", onDelete: .cascade))
            .field("name", .string, .required)
            .field("provider", .string, .required)
            .field("base_url", .string, .required)
            .field("model_id", .string, .required)
            .field("encrypted_api_key", .data, .required)
            .field("api_key_nonce", .data, .required)
            .field("created_at", .datetime)
            .field("updated_at", .datetime)
            .create()
    }

    func revert(on database: any Database) async throws {
        try await database.schema("provider_keys").delete()
    }
}
