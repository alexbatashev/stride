import Fluent

public struct CreateStoredChatThread: Migration {
    public init() {}

    public func prepare(on database: any Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatThread.schema)
            .id()
            .field("user_id", .uuid)
            .field("title", .string, .required)
            .field("created_at", .datetime, .required)
            .field("updated_at", .datetime, .required)
            .field("preview_text", .string, .required)
            .field("is_pinned", .bool, .required)
            .ignoreExisting()
            .create()
    }

    public func revert(on database: any Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatThread.schema).delete()
    }
}

public struct CreateStoredChatMessage: Migration {
    public init() {}

    public func prepare(on database: any Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatMessage.schema)
            .id()
            .field("thread_id", .uuid, .required)
            .field("user_id", .uuid)
            .field("parent_id", .uuid)
            .field("provider_id", .string, .required)
            .field("model_id", .string, .required)
            .field("model_name", .string, .required)
            .field("role", .string, .required)
            .field("thinking", .string)
            .field("content", .string, .required)
            .field("tool_call", .string)
            .field("tool_result", .string)
            .field("created_at", .datetime, .required)
            .field("updated_at", .datetime, .required)
            .field("is_done", .bool, .required)
            .field("usage", .string)
            .foreignKey("thread_id", references: StoredChatThread.schema, .id, onDelete: .cascade)
            .ignoreExisting()
            .create()
    }

    public func revert(on database: any Database) -> EventLoopFuture<Void> {
        database.schema(StoredChatMessage.schema).delete()
    }
}
