import Foundation
import SwiftData

// MARK: - Domain Enums

enum NoteBlockKind: String, Codable, CaseIterable, Sendable {
    case text
    case heading
    case checklist
    case table
    case image
    case drawing
    case attachment
    case code
    case quote
}

enum NoteAttachmentKind: String, Codable, CaseIterable, Sendable {
    case image
    case drawing
    case file
    case audio
    case video
}

// MARK: - SwiftData Models

@Model
final class Note {
    @Attribute(.unique) var id: UUID
    var title: String
    var createdAt: Date
    var updatedAt: Date
    var previewText: String
    var isPinned: Bool

    var blocks: [NoteBlock] = []

    init(
        id: UUID = UUID(),
        title: String = "New Note",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        previewText: String = "",
        isPinned: Bool = false
    ) {
        self.id = id
        self.title = title
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.previewText = previewText
        self.isPinned = isPinned
    }

    var orderedBlocks: [NoteBlock] {
        blocks.sorted {
            if $0.orderIndex == $1.orderIndex {
                return $0.createdAt < $1.createdAt
            }
            return $0.orderIndex < $1.orderIndex
        }
    }

    var nextOrderIndex: Int {
        (blocks.map(\.orderIndex).max() ?? -1) + 1
    }

    func refreshPreview() {
        let firstPreview = orderedBlocks
            .lazy
            .compactMap(\.plainTextPreview)
            .first(where: { !$0.isEmpty }) ?? ""

        previewText = String(firstPreview.prefix(100))
        updatedAt = .now

        let trimmedPreview = firstPreview.trimmingCharacters(in: .whitespacesAndNewlines)
        if title == "New Note", !trimmedPreview.isEmpty {
            title = String(trimmedPreview.prefix(36))
        }
    }
}

@Model
final class NoteBlock {
    @Attribute(.unique) var id: UUID
    var kindRawValue: String
    var orderIndex: Int
    var textContent: String
    var payloadJSON: String
    var createdAt: Date
    var updatedAt: Date

    var note: Note?

    var attachments: [NoteAttachment] = []

    init(
        id: UUID = UUID(),
        kind: NoteBlockKind,
        orderIndex: Int,
        textContent: String = "",
        payloadJSON: String = "{}",
        createdAt: Date = .now,
        updatedAt: Date = .now,
        note: Note? = nil
    ) {
        self.id = id
        self.kindRawValue = kind.rawValue
        self.orderIndex = orderIndex
        self.textContent = textContent
        self.payloadJSON = payloadJSON
        self.createdAt = createdAt
        self.updatedAt = updatedAt
        self.note = note
    }

    var kind: NoteBlockKind {
        get { NoteBlockKind(rawValue: kindRawValue) ?? .text }
        set { kindRawValue = newValue.rawValue }
    }

    var plainTextPreview: String? {
        let trimmed = textContent.trimmingCharacters(in: .whitespacesAndNewlines)
        if !trimmed.isEmpty {
            return trimmed
        }

        switch kind {
        case .table:
            return "Table"
        case .image:
            return "Image"
        case .drawing:
            return "Drawing"
        case .attachment:
            return "Attachment"
        case .checklist:
            return "Checklist"
        case .text, .heading, .code, .quote:
            return nil
        }
    }
}

@Model
final class NoteAttachment {
    @Attribute(.unique) var id: UUID
    var kindRawValue: String
    var fileName: String
    var mimeType: String
    var localPath: String
    var byteCount: Int
    var metadataJSON: String
    var createdAt: Date

    var block: NoteBlock?

    init(
        id: UUID = UUID(),
        kind: NoteAttachmentKind,
        fileName: String,
        mimeType: String,
        localPath: String,
        byteCount: Int,
        metadataJSON: String = "{}",
        createdAt: Date = .now,
        block: NoteBlock? = nil
    ) {
        self.id = id
        self.kindRawValue = kind.rawValue
        self.fileName = fileName
        self.mimeType = mimeType
        self.localPath = localPath
        self.byteCount = byteCount
        self.metadataJSON = metadataJSON
        self.createdAt = createdAt
        self.block = block
    }

    var kind: NoteAttachmentKind {
        get { NoteAttachmentKind(rawValue: kindRawValue) ?? .file }
        set { kindRawValue = newValue.rawValue }
    }
}
