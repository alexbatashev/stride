import Fluent
import Vapor
import FridayAPI

final class ConversationModel: Model, @unchecked Sendable {
    static let schema = "conversations"

    @ID(key: .id)
    var id: UUID?

    @Parent(key: "user_id")
    var user: UserModel

    @Field(key: "title")
    var title: String

    @Field(key: "preview_text")
    var previewText: String

    @Field(key: "is_pinned")
    var isPinned: Bool

    @Timestamp(key: "created_at", on: .create)
    var createdAt: Date?

    @Timestamp(key: "updated_at", on: .update)
    var updatedAt: Date?

    @Children(for: \.$conversation)
    var turns: [ConversationTurnModel]

    init() {}

    init(id: UUID? = nil, userID: UUID, title: String, previewText: String = "", isPinned: Bool = false) {
        self.id = id
        self.$user.id = userID
        self.title = title
        self.previewText = previewText
        self.isPinned = isPinned
    }

    func toDTO(with turns: [ConversationTurnDTO] = []) -> ConversationDTO {
        ConversationDTO(
            id: id!,
            title: title,
            createdAt: createdAt ?? Date(),
            updatedAt: updatedAt ?? Date(),
            previewText: previewText,
            isPinned: isPinned,
            turns: turns
        )
    }
}
