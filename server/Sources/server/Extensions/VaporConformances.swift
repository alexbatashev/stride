import Vapor
import FridayAPI

extension AuthResponse: @retroactive Content {}
extension ConversationDTO: @retroactive Content {}
extension ConversationTurnDTO: @retroactive Content {}
extension CreateConversationRequest: @retroactive Content {}
extension RegisterRequest: @retroactive Content {}
extension LoginRequest: @retroactive Content {}
