import CoreFriday
import SwiftUI

struct NotesListView: View {
    @Environment(ModelData.self) private var modelData

    var body: some View {
        @Bindable var modelData = modelData

        let notes = modelData.sortedNotes

        List(selection: $modelData.selectedNote) {
            ForEach(notes) { note in
                NavigationLink(value: note) {
                    NoteRow(note: note)
                }
            }
            .onDelete(perform: modelData.deleteNotes)
        }
        .navigationDestination(for: Note.self) { note in
            NoteDetailView(note: note)
        }
        .accessibilityIdentifier("notesList")
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
                Button(action: modelData.createNote) {
                    Label("New Note", systemImage: "square.and.pencil")
                }
                .help("New Note")
                .accessibilityIdentifier("newNoteButton")
            }
        }
        .onAppear(perform: modelData.ensureInitialNote)
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
