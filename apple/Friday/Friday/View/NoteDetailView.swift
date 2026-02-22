import CoreFriday
import SwiftUI

struct NoteDetailView: View {
    @Bindable var modelData: ModelData
    let noteID: UUID?

    private var selectedNote: Note? {
        if let noteID {
            return modelData.notes.first(where: { $0.id == noteID })
        }
        return modelData.sortedNotes.first
    }

    var body: some View {
        Group {
            if let note = selectedNote {
                VStack(spacing: 0) {
                    header(note: note)
                    Divider()
                    content(note: note)
                    Divider()
                    editorToolbar(note: note)
                }
                .background(
                    LinearGradient(
                        colors: [Color.accentColor.opacity(0.08), Color.clear],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
            } else {
                ContentUnavailableView(
                    "Select a Note",
                    systemImage: "note.text",
                    description: Text("Choose or create a note from the middle column.")
                )
            }
        }
    }

    private func header(note: Note) -> some View {
        HStack {
            TextField("Title", text: Binding(
                get: { note.title },
                set: {
                    note.title = $0
                    persist(note: note)
                }
            ))
            .textFieldStyle(.plain)
            .font(.title2.weight(.semibold))
            .accessibilityIdentifier("noteTitleField")

            Spacer()

            Text("Fluent + SQLite")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    private func content(note: Note) -> some View {
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                ForEach(note.orderedBlocks) { block in
                    NoteBlockCard(block: block) {
                        persist(note: note)
                    }
                }
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 18)
        }
        .accessibilityIdentifier("noteBlockList")
    }

    private func editorToolbar(note: Note) -> some View {
        HStack(spacing: 8) {
            Button(action: { addBlock(kind: .text, to: note) }) {
                Label("Text", systemImage: "text.alignleft")
            }
            .buttonStyle(.bordered)
            .accessibilityIdentifier("addTextBlockButton")

            Button(action: { addBlock(kind: .checklist, to: note) }) {
                Label("Checklist", systemImage: "checklist")
            }
            .buttonStyle(.bordered)

            Button(action: { addBlock(kind: .table, to: note) }) {
                Label("Table", systemImage: "tablecells")
            }
            .buttonStyle(.bordered)

            Button(action: { addDrawingBlock(to: note) }) {
                Label("Sketch", systemImage: "pencil.tip")
            }
            .buttonStyle(.bordered)

            Button(action: { addImageBlock(to: note) }) {
                Label("Image", systemImage: "photo")
            }
            .buttonStyle(.bordered)

            Spacer()
        }
        .padding(16)
        .background(.thinMaterial)
    }

    private func addBlock(kind: NoteBlockKind, to note: Note) {
        let seedText: String
        switch kind {
        case .text:
            seedText = ""
        case .heading:
            seedText = "Heading"
        case .checklist:
            seedText = "- [ ] First item"
        case .table:
            seedText = ""
        case .image:
            seedText = ""
        case .drawing:
            seedText = ""
        case .attachment:
            seedText = ""
        case .code:
            seedText = "// Code snippet"
        case .quote:
            seedText = "> Quote"
        }

        let block = NoteBlock(
            kind: kind,
            orderIndex: note.nextOrderIndex,
            textContent: seedText
        )
        note.blocks.append(block)

        persist(note: note)
    }

    private func addDrawingBlock(to note: Note) {
        let block = NoteBlock(
            kind: .drawing,
            orderIndex: note.nextOrderIndex,
            textContent: "",
            payloadJSON: "{\"canvas\":\"v1\"}"
        )
        note.blocks.append(block)

        let attachment = NoteAttachment(
            kind: .drawing,
            fileName: "Sketch-\(Int(Date.now.timeIntervalSince1970)).drawing",
            mimeType: "application/vnd.apple.notes.drawing",
            localPath: "/local/stub/sketch.drawing",
            byteCount: 2_048
        )
        block.attachments.append(attachment)

        persist(note: note)
    }

    private func addImageBlock(to note: Note) {
        let block = NoteBlock(
            kind: .image,
            orderIndex: note.nextOrderIndex,
            textContent: "",
            payloadJSON: "{\"layout\":\"inline\"}"
        )
        note.blocks.append(block)

        let attachment = NoteAttachment(
            kind: .image,
            fileName: "Image-\(Int(Date.now.timeIntervalSince1970)).jpg",
            mimeType: "image/jpeg",
            localPath: "/local/stub/image.jpg",
            byteCount: 85_000
        )
        block.attachments.append(attachment)

        persist(note: note)
    }

    private func persist(note: Note) {
        note.refreshPreview()
        modelData.persistAll()
    }
}

private struct NoteBlockCard: View {
    @Bindable var block: NoteBlock
    let onChange: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 8) {
            Label(blockTitle, systemImage: blockIcon)
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)

            switch block.kind {
            case .table:
                tableBlock
            case .image, .drawing, .attachment:
                attachmentBlock
            case .checklist:
                checklistBlock
            case .text, .heading, .code, .quote:
                textBlock
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(.ultraThinMaterial, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
    }

    private var textBlock: some View {
        TextField("Content", text: $block.textContent, axis: .vertical)
            .lineLimit(1...8)
            .textFieldStyle(.plain)
            .onChange(of: block.textContent) { _, _ in
                block.updatedAt = .now
                onChange()
            }
    }

    private var checklistBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            TextEditor(text: $block.textContent)
                .frame(minHeight: 72)
                .scrollContentBackground(.hidden)
                .onChange(of: block.textContent) { _, _ in
                    block.updatedAt = .now
                    onChange()
                }
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(.quaternary))
        }
    }

    private var tableBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Table block placeholder")
                .font(.subheadline.weight(.medium))
            Text("Rows and cells can be stored in payload JSON for a future structured editor.")
                .font(.caption)
                .foregroundStyle(.secondary)
            TextEditor(text: $block.payloadJSON)
                .font(.caption.monospaced())
                .frame(minHeight: 68)
                .scrollContentBackground(.hidden)
                .onChange(of: block.payloadJSON) { _, _ in
                    block.updatedAt = .now
                    onChange()
                }
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(.quaternary))
        }
    }

    private var attachmentBlock: some View {
        VStack(alignment: .leading, spacing: 6) {
            if block.attachments.isEmpty {
                Text("No attachments linked yet.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                ForEach(block.attachments) { attachment in
                    Label("\(attachment.fileName) · \(attachment.byteCount) B", systemImage: attachmentIcon(for: attachment.kind))
                        .font(.caption)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 6)
                        .background(.regularMaterial, in: Capsule())
                }
            }

            TextEditor(text: $block.payloadJSON)
                .font(.caption.monospaced())
                .frame(minHeight: 56)
                .scrollContentBackground(.hidden)
                .onChange(of: block.payloadJSON) { _, _ in
                    block.updatedAt = .now
                    onChange()
                }
                .overlay(RoundedRectangle(cornerRadius: 8, style: .continuous).stroke(.quaternary))
        }
    }

    private var blockTitle: String {
        switch block.kind {
        case .text:
            return "Text"
        case .heading:
            return "Heading"
        case .checklist:
            return "Checklist"
        case .table:
            return "Table"
        case .image:
            return "Image"
        case .drawing:
            return "Drawing"
        case .attachment:
            return "Attachment"
        case .code:
            return "Code"
        case .quote:
            return "Quote"
        }
    }

    private var blockIcon: String {
        switch block.kind {
        case .text:
            return "text.alignleft"
        case .heading:
            return "textformat.size.larger"
        case .checklist:
            return "checklist"
        case .table:
            return "tablecells"
        case .image:
            return "photo"
        case .drawing:
            return "pencil.tip"
        case .attachment:
            return "paperclip"
        case .code:
            return "chevron.left.forwardslash.chevron.right"
        case .quote:
            return "quote.bubble"
        }
    }

    private func attachmentIcon(for kind: NoteAttachmentKind) -> String {
        switch kind {
        case .image:
            return "photo"
        case .drawing:
            return "pencil.tip"
        case .file:
            return "doc"
        case .audio:
            return "waveform"
        case .video:
            return "film"
        }
    }
}
