import FileProvider

/// Enumerates the contents of one directory in the global library. The working
/// set (specially-tracked items) is intentionally empty — Friday has no
/// favorites/recents concept, so the system just re-reads directories on signal.
final class FileProviderEnumerator: NSObject, NSFileProviderEnumerator {
    private let containerPath: String
    private let isWorkingSet: Bool

    init(containerPath: String, isWorkingSet: Bool = false) {
        self.containerPath = containerPath
        self.isWorkingSet = isWorkingSet
    }

    func invalidate() {}

    func enumerateItems(for observer: NSFileProviderEnumerationObserver, startingAt page: NSFileProviderPage) {
        if isWorkingSet {
            observer.finishEnumerating(upTo: nil)
            return
        }
        guard let service = FridayFilesService() else {
            observer.finishEnumeratingWithError(NSFileProviderError(.notAuthenticated))
            return
        }
        let container = containerPath
        Task {
            do {
                let entries = try await service.list(path: container)
                let items = entries.map { FileProviderItem.from($0, parentPath: container) }
                observer.didEnumerate(items)
                observer.finishEnumerating(upTo: nil)
            } catch {
                observer.finishEnumeratingWithError(error)
            }
        }
    }

    func enumerateChanges(for observer: NSFileProviderChangeObserver, from anchor: NSFileProviderSyncAnchor) {
        // No incremental delta tracking; re-enumeration on signal keeps things
        // fresh. Report no changes against the current anchor.
        observer.finishEnumeratingChanges(upTo: anchor, moreComing: false)
    }

    func currentSyncAnchor(completionHandler: @escaping (NSFileProviderSyncAnchor?) -> Void) {
        completionHandler(NSFileProviderSyncAnchor(Data("0".utf8)))
    }
}
