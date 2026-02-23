import Fluent

struct CreateConversationTurn: AsyncMigration {
    func prepare(on database: any Database) async throws {
        try await database.schema("conversation_turns")
            .id()
            .field("conversation_id", .uuid, .required, .references("conversations", "id", onDelete: .cascade))
            .field("role", .string, .required)
            .field("text", .string, .required)
            .field("sequence_number", .int, .required)
            .field("model_identifier", .string)
            .field("is_error", .bool, .required)
            .field("attachments_json", .string, .required)
            .field("tool_invocations_json", .string, .required)
            .field("created_at", .datetime)
            .create()
    }

    func revert(on database: any Database) async throws {
        try await database.schema("conversation_turns").delete()
    }
}
