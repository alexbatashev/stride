import Fluent
import Vapor
import FridayAPI

final class ConversationTurnModel: Model, @unchecked Sendable {
    static let schema = "conversation_turns"

    @ID(key: .id)
    var id: UUID?

    @Parent(key: "conversation_id")
    var conversation: ConversationModel

    @Field(key: "role")
    var role: String

    @Field(key: "text")
    var text: String

    @Field(key: "sequence_number")
    var sequenceNumber: Int

    @OptionalField(key: "model_identifier")
    var modelIdentifier: String?

    @Field(key: "is_error")
    var isError: Bool

    @Field(key: "attachments_json")
    var attachmentsJSON: String

    @Field(key: "tool_invocations_json")
    var toolInvocationsJSON: String

    @Timestamp(key: "created_at", on: .create)
    var createdAt: Date?

    init() {}

    init(
        id: UUID? = nil,
        conversationID: UUID,
        role: String,
        text: String,
        sequenceNumber: Int,
        modelIdentifier: String? = nil,
        isError: Bool = false,
        attachmentsJSON: String = "[]",
        toolInvocationsJSON: String = "[]"
    ) {
        self.id = id
        self.$conversation.id = conversationID
        self.role = role
        self.text = text
        self.sequenceNumber = sequenceNumber
        self.modelIdentifier = modelIdentifier
        self.isError = isError
        self.attachmentsJSON = attachmentsJSON
        self.toolInvocationsJSON = toolInvocationsJSON
    }

    func toDTO() throws -> ConversationTurnDTO {
        let decoder = JSONDecoder()
        decoder.dateDecodingStrategy = .iso8601

        let attachments = try decoder.decode([TurnAttachment].self, from: Data(attachmentsJSON.utf8))
        let toolInvocations = try decoder.decode([ToolInvocation].self, from: Data(toolInvocationsJSON.utf8))

        return ConversationTurnDTO(
            id: id!,
            role: TurnRole(rawValue: role) ?? .user,
            text: text,
            createdAt: createdAt ?? Date(),
            sequenceNumber: sequenceNumber,
            modelIdentifier: modelIdentifier,
            isError: isError,
            attachments: attachments,
            toolInvocations: toolInvocations
        )
    }
}
