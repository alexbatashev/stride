import Vapor

func routes(_ app: Application) throws {
    app.get { req async in
        "It works!"
    }

    try app.register(collection: AuthController())
    try app.register(collection: ConversationController())
    try app.register(collection: APIKeyController())
    try app.register(collection: ProviderKeyController())
    try app.register(collection: LLMController())
}
