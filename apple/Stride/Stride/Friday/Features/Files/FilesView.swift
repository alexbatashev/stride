import ComposableArchitecture
import QuickLook
import SwiftUI
import UniformTypeIdentifiers

/// File browser used both as the standalone Files tab and the in-thread sidebar.
struct FilesView: View {
    @Bindable var store: StoreOf<FilesFeature>

    var body: some View {
        VStack(spacing: 0) {
            header
            Divider()
            content
        }
        .task { store.send(.onAppear) }
        .fileImporter(
            isPresented: $store.isImporting,
            allowedContentTypes: [.item],
            allowsMultipleSelection: true
        ) { result in
            switch result {
            case let .success(urls):
                store.send(.filesPicked(urls))
            case .failure:
                store.send(.setError("Couldn't import the selected files."))
            }
        }
        .quickLookPreview($store.previewURL)
        .alert("New Folder", isPresented: $store.newFolderShown) {
            TextField("Name", text: $store.newFolderName)
            Button("Create") { store.send(.createFolder) }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("Enter a name for the new folder.")
        }
        .alert("Rename", isPresented: $store.renameShown) {
            TextField("Name", text: $store.renameText)
            Button("Rename") { store.send(.confirmRename) }
            Button("Cancel", role: .cancel) {}
        }
        .alert(
            "Delete \(store.deleteTarget?.name ?? "Item")?",
            isPresented: deleteBinding,
            presenting: store.deleteTarget
        ) { _ in
            Button("Delete", role: .destructive) { store.send(.confirmDelete) }
            Button("Cancel", role: .cancel) {}
        } message: { entry in
            Text(entry.isDirectory
                ? "The folder and its contents will be removed."
                : "This file will be removed.")
        }
    }

    private var header: some View {
        VStack(spacing: 8) {
            HStack(spacing: 12) {
                if store.canGoUp {
                    Button { store.send(.goUp) } label: {
                        Image(systemName: "chevron.left")
                    }
                    .buttonStyle(.borderless)
                }

                Text(store.title)
                    .font(.headline)
                    .lineLimit(1)

                Spacer()

                Button { store.send(.newFolderTapped) } label: {
                    Image(systemName: "folder.badge.plus")
                }
                .buttonStyle(.borderless)
                .help("New Folder")

                Button { store.send(.uploadTapped) } label: {
                    Image(systemName: "square.and.arrow.up")
                }
                .buttonStyle(.borderless)
                .help("Upload")
            }

            if let error = store.errorMessage {
                HStack(spacing: 8) {
                    Image(systemName: "exclamationmark.triangle.fill")
                        .foregroundStyle(.orange)
                    Text(error)
                        .font(.footnote)
                        .foregroundStyle(.secondary)
                    Spacer()
                    Button("Dismiss") { store.send(.dismissError) }
                        .font(.footnote)
                }
            }
        }
        .padding(.horizontal, Metrics.gutter)
        .padding(.vertical, 12)
    }

    @ViewBuilder
    private var content: some View {
        if store.isLoading, store.entries.isEmpty {
            ProgressView()
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if store.entries.isEmpty {
            ContentUnavailableView {
                Label("No Files", systemImage: "folder")
            } description: {
                Text("Upload a file to get started.")
            } actions: {
                Button("Upload") { store.send(.uploadTapped) }
            }
        } else {
            List {
                ForEach(store.entries) { entry in
                    FileRow(entry: entry)
                        .contentShape(Rectangle())
                        .onTapGesture { store.send(.open(entry)) }
                        .contextMenu { rowMenu(entry) }
                        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                            Button(role: .destructive) {
                                store.send(.deleteTapped(entry))
                            } label: {
                                Label("Delete", systemImage: "trash")
                            }
                            if store.supportsRename {
                                Button {
                                    store.send(.renameTapped(entry))
                                } label: {
                                    Label("Rename", systemImage: "pencil")
                                }
                                .tint(.indigo)
                            }
                        }
                }
            }
            .listStyle(.plain)
            .refreshable { await store.send(.refresh).finish() }
        }
    }

    @ViewBuilder
    private func rowMenu(_ entry: FileEntry) -> some View {
        Button { store.send(.open(entry)) } label: {
            Label(entry.isDirectory ? "Open" : "Preview",
                  systemImage: entry.isDirectory ? "folder" : "eye")
        }
        if store.supportsRename {
            Button { store.send(.renameTapped(entry)) } label: {
                Label("Rename", systemImage: "pencil")
            }
        }
        Button(role: .destructive) { store.send(.deleteTapped(entry)) } label: {
            Label("Delete", systemImage: "trash")
        }
    }

    private var deleteBinding: Binding<Bool> {
        Binding(
            get: { store.deleteTarget != nil },
            set: { if !$0 { store.deleteTarget = nil } }
        )
    }
}

private struct FileRow: View {
    let entry: FileEntry

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: entry.isDirectory ? "folder.fill" : "doc")
                .font(.title3)
                .foregroundStyle(entry.isDirectory ? Color.accentColor : .secondary)
                .frame(width: 24)

            VStack(alignment: .leading, spacing: 2) {
                Text(entry.name)
                    .lineLimit(1)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }

            Spacer()

            if entry.isDirectory {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 4)
    }

    private var subtitle: String {
        let date = entry.updatedDate.formatted(date: .abbreviated, time: .omitted)
        if entry.isDirectory { return date }
        let size = entry.sizeLabel
        return size.isEmpty ? date : "\(size) · \(date)"
    }
}
