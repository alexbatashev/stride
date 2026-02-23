import Vapor
import Fluent
import FluentSQLiteDriver
import JWT

public func configure(_ app: Application) async throws {
    // JSON encoder/decoder with ISO 8601 date strategy
    let encoder = JSONEncoder()
    encoder.dateEncodingStrategy = .iso8601
    let decoder = JSONDecoder()
    decoder.dateDecodingStrategy = .iso8601
    ContentConfiguration.global.use(encoder: encoder, for: .json)
    ContentConfiguration.global.use(decoder: decoder, for: .json)

    // SQLite database
    app.databases.use(.sqlite(.file("friday.sqlite")), as: .sqlite)

    // JWT signers
    app.jwt.signers.use(.hs256(key: Environment.get("JWT_SECRET") ?? "dev-secret-change-in-prod"))

    // Migrations
    app.migrations.add(CreateUser())
    app.migrations.add(CreateConversation())
    app.migrations.add(CreateConversationTurn())
    app.migrations.add(CreateAPIKey())
    app.migrations.add(CreateProviderKey())

    try await app.autoMigrate()

    // Routes
    try routes(app)
}
