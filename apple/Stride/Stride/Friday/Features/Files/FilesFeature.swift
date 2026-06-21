import ComposableArchitecture
import Foundation
import UniformTypeIdentifiers

/// Browses one file namespace (global library or a thread workspace). Reused by
/// the standalone Files tab and the in-thread file sidebar.
@Reducer
struct FilesFeature {
    @ObservableState
    struct State: Equatable {
        let scope: FileScope
        var path: String = ""
        var entries: IdentifiedArrayOf<FileEntry> = []
        var isLoading = false
        var errorMessage: String?

        var isImporting = false
        var previewURL: URL?

        var newFolderShown = false
        var newFolderName = ""

        var renameShown = false
        var renameTarget: FileEntry?
        var renameText = ""

        var deleteTarget: FileEntry?

        /// Only the global library exposes a rename endpoint.
        var supportsRename: Bool { scope == .global }
        var canGoUp: Bool { !path.isEmpty }

        /// Last path segment, or a scope-appropriate root label.
        var title: String {
            if let last = path.split(separator: "/").last { return String(last) }
            switch scope {
            case .global: return "Files"
            case .workspace: return "Workspace"
            }
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case onAppear
        case refresh
        case load
        case loaded(Result<FileListing, FridayError>)
        case open(FileEntry)
        case previewReady(URL)
        case goUp
        case newFolderTapped
        case createFolder
        case renameTapped(FileEntry)
        case confirmRename
        case deleteTapped(FileEntry)
        case confirmDelete
        case uploadTapped
        case filesPicked([URL])
        case mutationFinished(Result<Void, FridayError>)
        case setError(String?)
        case dismissError
    }

    @Dependency(\.friday) var friday

    private enum CancelID { case load }

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding:
                return .none

            case .onAppear:
                state.isLoading = state.entries.isEmpty
                return .send(.load)

            case .refresh:
                return .send(.load)

            case .load:
                let scope = state.scope
                let path = state.path
                return .run { send in
                    await send(.loaded(await asyncResult { try await friday.listFiles(scope, path) }))
                }
                .cancellable(id: CancelID.load, cancelInFlight: true)

            case let .loaded(.success(listing)):
                state.isLoading = false
                state.errorMessage = nil
                state.path = listing.path
                state.entries = IdentifiedArray(uniqueElements: sorted(listing.entries))
                return .none

            case let .loaded(.failure(error)):
                state.isLoading = false
                state.errorMessage = message(for: error)
                return .none

            case let .open(entry):
                if entry.isDirectory {
                    state.path = entry.path
                    state.isLoading = true
                    return .send(.load)
                }
                return downloadForPreview(scope: state.scope, entry: entry)

            case let .previewReady(url):
                state.previewURL = url
                return .none

            case .goUp:
                guard state.canGoUp else { return .none }
                var segments = state.path.split(separator: "/")
                segments.removeLast()
                state.path = segments.joined(separator: "/")
                state.isLoading = true
                return .send(.load)

            case .newFolderTapped:
                state.newFolderName = ""
                state.newFolderShown = true
                return .none

            case .createFolder:
                let name = state.newFolderName.trimmingCharacters(in: .whitespacesAndNewlines)
                state.newFolderShown = false
                guard !name.isEmpty else { return .none }
                let scope = state.scope
                let target = joined(state.path, name)
                return .run { send in
                    await send(.mutationFinished(await asyncResult { try await friday.createDirectory(scope, target) }))
                }

            case let .renameTapped(entry):
                state.renameTarget = entry
                state.renameText = entry.name
                state.renameShown = true
                return .none

            case .confirmRename:
                let newName = state.renameText.trimmingCharacters(in: .whitespacesAndNewlines)
                state.renameShown = false
                guard let target = state.renameTarget, !newName.isEmpty, newName != target.name else {
                    state.renameTarget = nil
                    return .none
                }
                state.renameTarget = nil
                let path = target.path
                return .run { send in
                    await send(.mutationFinished(await asyncResult { try await friday.renameFile(path, newName) }))
                }

            case let .deleteTapped(entry):
                state.deleteTarget = entry
                return .none

            case .confirmDelete:
                guard let target = state.deleteTarget else { return .none }
                state.deleteTarget = nil
                let scope = state.scope
                let path = target.path
                return .run { send in
                    await send(.mutationFinished(await asyncResult { try await friday.deleteFile(scope, path) }))
                }

            case .uploadTapped:
                state.isImporting = true
                return .none

            case let .filesPicked(urls):
                guard !urls.isEmpty else { return .none }
                let scope = state.scope
                let directory = state.path
                return .run { send in
                    let files = readUploads(from: urls)
                    guard !files.isEmpty else {
                        await send(.setError("Couldn't read the selected files."))
                        return
                    }
                    await send(.mutationFinished(await asyncResult {
                        _ = try await friday.uploadFiles(scope, directory, files)
                    }))
                }

            case .mutationFinished(.success):
                state.isLoading = true
                if state.scope == .global {
                    FridayProviderBridge.shared.signalChange()
                }
                return .send(.load)

            case let .mutationFinished(.failure(error)):
                state.errorMessage = message(for: error)
                return .none

            case let .setError(message):
                state.errorMessage = message
                return .none

            case .dismissError:
                state.errorMessage = nil
                return .none
            }
        }
    }

    private func downloadForPreview(scope: FileScope, entry: FileEntry) -> Effect<Action> {
        .run { send in
            do {
                let data = try await friday.downloadFile(scope, entry.path)
                let url = try writeTemporaryFile(named: entry.name, data: data)
                await send(.previewReady(url))
            } catch {
                await send(.setError("Couldn't open \(entry.name)."))
            }
        }
    }

    private func sorted(_ entries: [FileEntry]) -> [FileEntry] {
        entries.sorted { lhs, rhs in
            if lhs.isDirectory != rhs.isDirectory { return lhs.isDirectory }
            return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
        }
    }

    private func joined(_ parent: String, _ name: String) -> String {
        parent.isEmpty ? name : "\(parent)/\(name)"
    }

    private func message(for error: FridayError) -> String {
        switch error {
        case .unauthorized: return "Your session expired. Sign in again."
        case .notConfigured: return "No server configured."
        default: return "Something went wrong. Try again."
        }
    }
}

/// Runs an async throwing call and captures the outcome as a `Result`, since
/// `Result(catching:)` accepts only synchronous closures.
private func asyncResult<T>(_ body: () async throws -> T) async -> Result<T, FridayError> {
    do {
        return .success(try await body())
    } catch {
        return .failure(FridayError.from(error))
    }
}

/// Reads the bytes and metadata of files chosen from the document picker. Picked
/// URLs are security-scoped, so access must be claimed before reading.
private func readUploads(from urls: [URL]) -> [FileUpload] {
    urls.compactMap { url in
        let scoped = url.startAccessingSecurityScopedResource()
        defer { if scoped { url.stopAccessingSecurityScopedResource() } }
        guard let data = try? Data(contentsOf: url) else { return nil }
        let mime = UTType(filenameExtension: url.pathExtension)?.preferredMIMEType
        return FileUpload(name: url.lastPathComponent, mimeType: mime, data: data)
    }
}

/// Writes downloaded bytes to a unique temp location so QuickLook (and its share
/// sheet) can present and export them.
private func writeTemporaryFile(named name: String, data: Data) throws -> URL {
    let directory = FileManager.default.temporaryDirectory
        .appendingPathComponent("friday-preview", isDirectory: true)
        .appendingPathComponent(UUID().uuidString, isDirectory: true)
    try FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
    let url = directory.appendingPathComponent(name.isEmpty ? "file" : name)
    try data.write(to: url, options: .atomic)
    return url
}

extension FridayError {
    /// Normalizes an arbitrary thrown error into a `FridayError` for reducer use.
    static func from(_ error: Error) -> FridayError {
        (error as? FridayError) ?? .transport
    }
}
