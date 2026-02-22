import SwiftUI
import SwiftData

struct NotesListView: View {
    @Bindable var modelData: ModelData

    @Environment(\.modelContext) private var modelContext
    @Query(sort: [SortDescriptor(\Note.updatedAt, order: .reverse)])
    private var notes: [Note]

    var body: some View {
        List(selection: $modelData.selectedNoteID) {
            ForEach(notes) { note in
                NoteRow(note: note)
                    .tag(Optional(note.id))
            }
            .onDelete(perform: deleteNotes)
        }
        .accessibilityIdentifier("notesList")
        .navigationTitle("Notes")
        .overlay {
            if notes.isEmpty {
                ContentUnavailableView(
                    "No Notes",
                    systemImage: "note.text",
                    description: Text("Create a note to get started.")
                )
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button(action: createNote) {
                    Label("New Note", systemImage: "square.and.pencil")
                }
                .help("New Note")
                .accessibilityIdentifier("newNoteButton")
            }
        }
        .onAppear(perform: ensureInitialNote)
    }

    private func ensureInitialNote() {
        guard notes.isEmpty else {
            if modelData.selectedNoteID == nil {
                modelData.selectedNoteID = notes.first?.id
            }
            return
        }

        let note = Note(title: "Welcome")
        modelContext.insert(note)

        let block = NoteBlock(
            kind: .text,
            orderIndex: 0,
            textContent: "Welcome to Notes. This prototype stores flexible block-based note data in SwiftData.",
            note: note
        )
        note.blocks.append(block)
        note.refreshPreview()
        modelContext.insert(block)

        do {
            try modelContext.save()
            modelData.selectedNoteID = note.id
        } catch {
            assertionFailure("Failed to seed initial note: \(error)")
        }
    }

    private func createNote() {
        let note = Note()
        modelContext.insert(note)

        let block = NoteBlock(
            kind: .text,
            orderIndex: note.nextOrderIndex,
            textContent: "",
            note: note
        )
        note.blocks.append(block)
        modelContext.insert(block)

        note.refreshPreview()

        do {
            try modelContext.save()
            modelData.selectedNoteID = note.id
        } catch {
            assertionFailure("Failed to create note: \(error)")
        }
    }

    private func deleteNotes(at offsets: IndexSet) {
        for offset in offsets {
            let note = notes[offset]
            for block in note.blocks {
                for attachment in block.attachments {
                    modelContext.delete(attachment)
                }
                modelContext.delete(block)
            }
            modelContext.delete(note)
        }

        do {
            try modelContext.save()
            modelData.selectedNoteID = nil
        } catch {
            assertionFailure("Failed to delete note: \(error)")
        }
    }
}

private struct NoteRow: View {
    let note: Note

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(note.title)
                .font(.headline)
                .lineLimit(1)

            if !note.previewText.isEmpty {
                Text(note.previewText)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Text(note.updatedAt, style: .relative)
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 2)
    }
}
