import FileProvider
import UniformTypeIdentifiers

/// Replicated File Provider extension that surfaces the user's global Stride
/// library (`/api/files`) as a location in the Files app. Thread workspaces are
/// handled in-app and are intentionally not exposed here.
final class FileProviderExtension: NSObject, NSFileProviderReplicatedExtension {
    private let domain: NSFileProviderDomain

    required init(domain: NSFileProviderDomain) {
        self.domain = domain
        super.init()
    }

    func invalidate() {}

    func item(
        for identifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        if identifier == .rootContainer || identifier == .trashContainer {
            completionHandler(FileProviderItem.root(), nil)
            progress.completedUnitCount = 1
            return progress
        }
        guard let service = StrideFilesService() else {
            completionHandler(nil, NSFileProviderError(.notAuthenticated))
            return progress
        }
        let path = ItemID.path(for: identifier)
        let parent = ItemID.parentPath(of: path)
        Task {
            do {
                let entries = try await service.list(path: parent)
                guard let entry = entries.first(where: { $0.path == path }) else {
                    completionHandler(nil, NSFileProviderError(.noSuchItem))
                    progress.completedUnitCount = 1
                    return
                }
                completionHandler(FileProviderItem.from(entry, parentPath: parent), nil)
            } catch {
                completionHandler(nil, error)
            }
            progress.completedUnitCount = 1
        }
        return progress
    }

    func fetchContents(
        for itemIdentifier: NSFileProviderItemIdentifier,
        version requestedVersion: NSFileProviderItemVersion?,
        request: NSFileProviderRequest,
        completionHandler: @escaping (URL?, NSFileProviderItem?, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        guard let service = StrideFilesService() else {
            completionHandler(nil, nil, NSFileProviderError(.notAuthenticated))
            return progress
        }
        let path = ItemID.path(for: itemIdentifier)
        let parent = ItemID.parentPath(of: path)
        Task {
            do {
                let data = try await service.read(path: path)
                let url = try providerTemporaryFile(named: (path as NSString).lastPathComponent, data: data)
                let entries = try await service.list(path: parent)
                let item = entries.first(where: { $0.path == path })
                    .map { FileProviderItem.from($0, parentPath: parent) }
                completionHandler(url, item, nil)
            } catch {
                completionHandler(nil, nil, error)
            }
            progress.completedUnitCount = 1
        }
        return progress
    }

    func createItem(
        basedOn itemTemplate: NSFileProviderItem,
        fields: NSFileProviderItemFields,
        contents url: URL?,
        options: NSFileProviderCreateItemOptions,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        guard let service = StrideFilesService() else {
            completionHandler(nil, [], false, NSFileProviderError(.notAuthenticated))
            return progress
        }
        let parentPath = ItemID.path(for: itemTemplate.parentItemIdentifier)
        let name = itemTemplate.filename
        let newPath = parentPath.isEmpty ? name : "\(parentPath)/\(name)"
        let isDirectory = itemTemplate.contentType?.conforms(to: .folder) ?? false
        Task {
            do {
                if isDirectory {
                    try await service.createDirectory(path: newPath)
                } else {
                    let data = url.flatMap { try? Data(contentsOf: $0) } ?? Data()
                    try await service.upload(
                        directory: parentPath,
                        name: name,
                        data: data,
                        mime: itemTemplate.contentType?.preferredMIMEType
                    )
                }
                let entries = try await service.list(path: parentPath)
                guard let entry = entries.first(where: { $0.path == newPath }) else {
                    completionHandler(nil, [], false, NSFileProviderError(.noSuchItem))
                    progress.completedUnitCount = 1
                    return
                }
                completionHandler(FileProviderItem.from(entry, parentPath: parentPath), [], false, nil)
            } catch {
                completionHandler(nil, [], false, error)
            }
            progress.completedUnitCount = 1
        }
        return progress
    }

    func modifyItem(
        _ item: NSFileProviderItem,
        baseVersion version: NSFileProviderItemVersion,
        changedFields: NSFileProviderItemFields,
        contents newContents: URL?,
        options: NSFileProviderModifyItemOptions,
        request: NSFileProviderRequest,
        completionHandler: @escaping (NSFileProviderItem?, NSFileProviderItemFields, Bool, Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        guard let service = StrideFilesService() else {
            completionHandler(nil, [], false, NSFileProviderError(.notAuthenticated))
            return progress
        }
        let path = ItemID.path(for: item.itemIdentifier)
        let parentPath = ItemID.parentPath(of: path)
        Task {
            do {
                var currentPath = path
                if changedFields.contains(.filename) {
                    try await service.rename(path: currentPath, to: item.filename)
                    currentPath = parentPath.isEmpty ? item.filename : "\(parentPath)/\(item.filename)"
                }
                if changedFields.contains(.contents), let newContents {
                    let data = (try? Data(contentsOf: newContents)) ?? Data()
                    try await service.upload(
                        directory: parentPath,
                        name: (currentPath as NSString).lastPathComponent,
                        data: data,
                        mime: item.contentType?.preferredMIMEType
                    )
                }
                let entries = try await service.list(path: parentPath)
                let resolved = entries.first(where: { $0.path == currentPath })
                    .map { FileProviderItem.from($0, parentPath: parentPath) } ?? item
                completionHandler(resolved, [], false, nil)
            } catch {
                completionHandler(nil, [], false, error)
            }
            progress.completedUnitCount = 1
        }
        return progress
    }

    func deleteItem(
        identifier: NSFileProviderItemIdentifier,
        baseVersion version: NSFileProviderItemVersion,
        options: NSFileProviderDeleteItemOptions,
        request: NSFileProviderRequest,
        completionHandler: @escaping (Error?) -> Void
    ) -> Progress {
        let progress = Progress(totalUnitCount: 1)
        guard let service = StrideFilesService() else {
            completionHandler(NSFileProviderError(.notAuthenticated))
            return progress
        }
        let path = ItemID.path(for: identifier)
        Task {
            do {
                try await service.delete(path: path)
                completionHandler(nil)
            } catch {
                completionHandler(error)
            }
            progress.completedUnitCount = 1
        }
        return progress
    }

    func enumerator(
        for containerItemIdentifier: NSFileProviderItemIdentifier,
        request: NSFileProviderRequest
    ) throws -> NSFileProviderEnumerator {
        if containerItemIdentifier == .workingSet || containerItemIdentifier == .trashContainer {
            return FileProviderEnumerator(containerPath: "", isWorkingSet: true)
        }
        return FileProviderEnumerator(containerPath: ItemID.path(for: containerItemIdentifier))
    }
}
