import Foundation
#if canImport(FileProvider)
import FileProvider
#endif

/// Bridges the in-app session to the system File Provider so the user's global
/// files appear as a location in the Files app. Every method is a no-op on
/// platforms without FileProvider or when the extension isn't installed yet.
final class StrideProviderBridge: @unchecked Sendable {
    static let shared = StrideProviderBridge()

    private init() {}

    #if canImport(FileProvider)
    static let domainIdentifier = NSFileProviderDomainIdentifier("me.batashev.Stride.stride-files")

    private var domain: NSFileProviderDomain {
        NSFileProviderDomain(identifier: Self.domainIdentifier, displayName: "S.T.R.I.D.E.")
    }
    #endif

    /// Registers the Files-app location. Call after a successful sign-in.
    func register() {
        #if canImport(FileProvider)
        NSFileProviderManager.add(domain) { _ in }
        #endif
    }

    /// Removes the Files-app location. Call after sign-out.
    func unregister() {
        #if canImport(FileProvider)
        NSFileProviderManager.remove(domain) { _ in }
        #endif
    }

    /// Asks the Files app to re-enumerate after an in-app change so the two stay
    /// in sync without waiting for a manual pull-to-refresh.
    func signalChange() {
        #if canImport(FileProvider)
        NSFileProviderManager(for: domain)?.signalEnumerator(for: .workingSet) { _ in }
        #endif
    }
}
