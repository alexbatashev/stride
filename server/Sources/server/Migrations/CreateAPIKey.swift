import Fluent

struct CreateAPIKey: AsyncMigration {
    func prepare(on database: any Database) async throws {
        try await database.schema("api_keys")
            .id()
            .field("user_id", .uuid, .required, .references("users", "id", onDelete: .cascade))
            .field("name", .string, .required)
            .field("key_hash", .string, .required)
            .field("key_prefix", .string, .required)
            .field("created_at", .datetime)
            .field("last_used_at", .datetime)
            .unique(on: "key_hash")
            .create()
    }

    func revert(on database: any Database) async throws {
        try await database.schema("api_keys").delete()
    }
}
