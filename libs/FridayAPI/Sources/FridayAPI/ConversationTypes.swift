import Foundation

public enum TurnRole: String, Codable, CaseIterable, Sendable {
    case user
    case assistant
    case tool
    case system
}

public enum AttachmentKind: String, Codable, CaseIterable, Sendable {
    case image
    case file
    case audio
    case video
}

public enum ToolInvocationStatus: String, Codable, CaseIterable, Sendable {
    case queued
    case running
    case completed
    case failed
}

public struct TurnAttachment: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID
    public var kind: AttachmentKind
    public var fileName: String
    public var mimeType: String
    public var localPath: String
    public var byteCount: Int
    public var createdAt: Date

    public init(
        id: UUID = UUID(),
        kind: AttachmentKind,
        fileName: String,
        mimeType: String,
        localPath: String,
        byteCount: Int,
        createdAt: Date = Date()
    ) {
        self.id = id
        self.kind = kind
        self.fileName = fileName
        self.mimeType = mimeType
        self.localPath = localPath
        self.byteCount = byteCount
        self.createdAt = createdAt
    }
}

public struct ToolInvocation: Identifiable, Codable, Equatable, Sendable {
    public var id: UUID
    public var name: String
    public var argumentsJSON: String
    public var resultJSON: String?
    public var status: ToolInvocationStatus
    public var startedAt: Date
    public var endedAt: Date?

    public init(
        id: UUID = UUID(),
        name: String,
        argumentsJSON: String,
        resultJSON: String? = nil,
        status: ToolInvocationStatus,
        startedAt: Date = Date(),
        endedAt: Date? = nil
    ) {
        self.id = id
        self.name = name
        self.argumentsJSON = argumentsJSON
        self.resultJSON = resultJSON
        self.status = status
        self.startedAt = startedAt
        self.endedAt = endedAt
    }
}
