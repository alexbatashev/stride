import CoreFriday
import Foundation
import JSKit

#if canImport(Glibc)
import Glibc
#elseif canImport(Darwin)
import Darwin
#endif

@inline(__always)
private func copyCString(_ value: String) -> UnsafeMutablePointer<CChar>? {
    value.withCString { strdup($0) }
}

@_cdecl("friday_bridge_string_free")
public func friday_bridge_string_free(_ pointer: UnsafeMutablePointer<CChar>?) {
    guard let pointer else { return }
    free(pointer)
}

@_cdecl("friday_bridge_jskit_eval")
public func friday_bridge_jskit_eval(_ source: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    guard let source else {
        return copyCString("Input source is null")
    }

    do {
        let runtime = try JavaScriptRuntime()
        let context = try runtime.makeContext()
        let result = try context.evaluate(String(cString: source))
        return copyCString(try result.string())
    } catch {
        return copyCString("JSKit error: \(error)")
    }
}

@_cdecl("friday_bridge_corefriday_snapshot_counts")
public func friday_bridge_corefriday_snapshot_counts(_ databasePath: UnsafePointer<CChar>?) -> UnsafeMutablePointer<CChar>? {
    guard let databasePath else {
        return copyCString("0,0")
    }

    do {
        let storage = try CoreFridayStorage(databaseFilePath: String(cString: databasePath))
        let snapshot = try storage.loadSnapshot()
        return copyCString("\(snapshot.conversations.count),\(snapshot.notes.count)")
    } catch {
        return copyCString("error:\(error)")
    }
}
