import ComposableArchitecture
import Foundation

/// Errors surfaced to features. `unauthorized` drives an automatic return to the
/// login screen; everything else becomes an inline banner.
enum FridayError: Error, Equatable {
    case notConfigured
    case unauthorized
    case http(Int)
    case transport
}

/// The full surface the app needs from the Friday cloud server, modelled as a
/// struct of closures so it can be swapped for previews and tests.
struct FridayClient {
    var login: @Sendable (_ baseURL: URL, _ username: String, _ password: String) async throws -> Void
    var register: @Sendable (_ baseURL: URL, _ username: String, _ password: String) async throws -> Void
    var signOut: @Sendable () async -> Void

    var listProjects: @Sendable () async throws -> [Project]
    var listThreads: @Sendable () async throws -> [ThreadSummary]
    var listMessages: @Sendable (_ threadID: String) async throws -> [Message]

    var createThread: @Sendable (_ content: String, _ projectID: String?, _ filePaths: [String]) async throws -> SendResult
    var sendMessage: @Sendable (_ threadID: String, _ content: String, _ filePaths: [String]) async throws -> SendResult
    var cancelRun: @Sendable (_ threadID: String) async throws -> Void
    var resolveApproval: @Sendable (_ threadID: String, _ approvalID: String, _ approved: Bool) async throws -> Void
    var answerQuiz: @Sendable (_ threadID: String, _ quizID: String, _ answers: [String]) async throws -> Void

    var events: @Sendable (_ threadID: String) -> AsyncThrowingStream<ThreadEvent, Error>

    var listFiles: @Sendable (_ scope: FileScope, _ path: String) async throws -> FileListing
    var createDirectory: @Sendable (_ scope: FileScope, _ path: String) async throws -> Void
    var renameFile: @Sendable (_ path: String, _ newName: String) async throws -> Void
    var deleteFile: @Sendable (_ scope: FileScope, _ path: String) async throws -> Void
    var uploadFiles: @Sendable (_ scope: FileScope, _ directory: String, _ files: [FileUpload]) async throws -> [UploadedFile]
    var downloadFile: @Sendable (_ scope: FileScope, _ path: String) async throws -> Data
}

extension FridayClient: DependencyKey {
    static let liveValue: FridayClient = .live(session: .shared)

    static let testValue = FridayClient(
        login: { _, _, _ in },
        register: { _, _, _ in },
        signOut: {},
        listProjects: { [] },
        listThreads: { [] },
        listMessages: { _ in [] },
        createThread: { _, _, _ in SendResult(threadID: "preview", runID: "run") },
        sendMessage: { _, _, _ in SendResult(threadID: "preview", runID: "run") },
        cancelRun: { _ in },
        resolveApproval: { _, _, _ in },
        answerQuiz: { _, _, _ in },
        events: { _ in AsyncThrowingStream { $0.finish() } },
        listFiles: { _, _ in FileListing(path: "", entries: []) },
        createDirectory: { _, _ in },
        renameFile: { _, _ in },
        deleteFile: { _, _ in },
        uploadFiles: { _, _, _ in [] },
        downloadFile: { _, _ in Data() }
    )
}

extension DependencyValues {
    var friday: FridayClient {
        get { self[FridayClient.self] }
        set { self[FridayClient.self] = newValue }
    }
}
