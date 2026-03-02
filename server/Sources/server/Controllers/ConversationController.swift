import Vapor
import Fluent
import JWT
import FridayAPI

struct ConversationController: RouteCollection {
    func boot(routes: any RoutesBuilder) throws {
        let protected = routes
            .grouped("api", "conversations")
            .grouped(UserPayload.authenticator(), UserPayload.guardMiddleware())

        protected.get(use: list)
        protected.post(use: create)
        protected.get(":id", use: show)
    }

    @Sendable
    func list(req: Request) async throws -> [ConversationDTO] {
        let payload = try req.auth.require(UserPayload.self)

        let conversations = try await ConversationModel.query(on: req.db)
            .filter(\.$user.$id == payload.userID)
            .all()

        return conversations.map { $0.toDTO() }
    }

    @Sendable
    func create(req: Request) async throws -> ConversationDTO {
        let payload = try req.auth.require(UserPayload.self)
        let body = try req.content.decode(CreateConversationRequest.self)

        let conversation = ConversationModel(
            id: body.id,
            userID: payload.userID,
            title: body.title,
            isPinned: body.isPinned
        )
        try await conversation.save(on: req.db)

        return conversation.toDTO()
    }

    @Sendable
    func show(req: Request) async throws -> ConversationDTO {
        let payload = try req.auth.require(UserPayload.self)

        guard let id = req.parameters.get("id", as: UUID.self) else {
            throw Abort(.badRequest)
        }

        guard let conversation = try await ConversationModel.query(on: req.db)
            .filter(\.$id == id)
            .filter(\.$user.$id == payload.userID)
            .with(\.$turns)
            .first()
        else {
            throw Abort(.notFound)
        }

        let turnDTOs = try conversation.turns
            .sorted { ($0.createdAt ?? .distantPast) < ($1.createdAt ?? .distantPast) }
            .map { try $0.toDTO() }

        return conversation.toDTO(with: turnDTOs)
    }
}
