import ComposableArchitecture
import Foundation

/// Errors surfaced to features. `unauthorized` drives an automatic return to the
/// login screen; everything else becomes an inline banner.
enum StrideError: Error, Equatable {
    case notConfigured
    case unauthorized
    case http(Int)
    case transport
}

/// The full surface the app needs from the Stride cloud server, modelled as a
/// struct of closures so it can be swapped for previews and tests.
struct StrideClient {
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

    var listAutomations: @Sendable () async throws -> [Automation]
    var createAutomation: @Sendable (_ automation: NewAutomation) async throws -> Automation
    var runAutomation: @Sendable (_ id: String) async throws -> Void
    var setAutomationEnabled: @Sendable (_ id: String, _ enabled: Bool) async throws -> Void
    var deleteAutomation: @Sendable (_ id: String) async throws -> Void
    var listAutomationRuns: @Sendable (_ id: String) async throws -> [AutomationRun]
    var listEmailAccounts: @Sendable () async throws -> [EmailAccount]

    /// The active server base URL, used to build the public webhook URL shown
    /// after a webhook automation is created.
    var serverBaseURL: @Sendable () -> URL?
}

extension StrideClient: DependencyKey {
    static let liveValue: StrideClient = .live(session: .shared)

    static let testValue = StrideClient(
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
        downloadFile: { _, _ in Data() },
        listAutomations: { [] },
        createAutomation: { _ in throw StrideError.notConfigured },
        runAutomation: { _ in },
        setAutomationEnabled: { _, _ in },
        deleteAutomation: { _ in },
        listAutomationRuns: { _ in [] },
        listEmailAccounts: { [] },
        serverBaseURL: { URL(string: "https://preview.stride.app") }
    )
}

extension DependencyValues {
    var stride: StrideClient {
        get { self[StrideClient.self] }
        set { self[StrideClient.self] = newValue }
    }
}
