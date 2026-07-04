import Foundation

actor LocalOperatorClient {
    static let shared = LocalOperatorClient()

    private struct LocalPendingApproval {
        let id: String
        let message: String
    }

    private struct ThreadState {
        var summary: ThreadSummary
        var messages: [Message]
        let runtime: any LocalOperatorThreadRuntime
        var eventSeq = 0
        var running = false
        var pendingApproval: LocalPendingApproval?
    }

    private var threads: [String: ThreadState] = [:]
    private var subscribers: [String: [UUID: AsyncThrowingStream<ThreadEvent, Error>.Continuation]] = [:]
    private let runtimeFactory = LocalOperatorRuntimeFactory()

    func listThreads() -> [ThreadSummary] {
        threads.values
            .map(\.summary)
            .sorted { $0.id > $1.id }
    }

    func listMessages(threadID: String) -> [Message] {
        threads[threadID]?.messages ?? []
    }

    func createThread(content: String, session: Session) async throws -> SendResult {
        let runtime = try await runtimeFactory.makeThread(initialContent: content, session: session)
        let id = runtime.id
        let user = makeMessage(seq: 0, role: .user, content: content)
        let title = runtime.title.isEmpty ? LocalOperatorRuntimeFactory.fallbackTitle(content) : runtime.title
        threads[id] = ThreadState(
            summary: ThreadSummary(id: id, title: title, projectID: nil, location: .local),
            messages: [user],
            runtime: runtime
        )
        startRun(threadID: id, content: content, session: session)

        return SendResult(threadID: id, runID: UUID().uuidString)
    }

    func sendMessage(threadID: String, content: String, session: Session) async throws -> SendResult {
        guard threads[threadID] != nil else { throw StrideError.http(404) }

        let seq = threads[threadID]?.messages.count ?? 0
        threads[threadID]?.messages.append(makeMessage(seq: seq, role: .user, content: content))
        startRun(threadID: threadID, content: content, session: session)

        return SendResult(threadID: threadID, runID: UUID().uuidString)
    }

    func resolveApproval(threadID: String, approvalID: String, approved: Bool) async {
        guard let runtime = threads[threadID]?.runtime else { return }
        _ = await runtime.resolveApproval(id: approvalID, approved: approved)
    }

    nonisolated func events(threadID: String) -> AsyncThrowingStream<ThreadEvent, Error> {
        AsyncThrowingStream { continuation in
            let id = UUID()
            Task { await self.addSubscriber(id, threadID: threadID, continuation: continuation) }
            continuation.onTermination = { _ in
                Task { await self.removeSubscriber(id, threadID: threadID) }
            }
        }
    }

    private func startRun(threadID: String, content: String, session: Session) {
        Task { await run(threadID: threadID, content: content, session: session) }
    }

    private func run(threadID: String, content: String, session: Session) async {
        guard let state = threads[threadID] else { return }
        let runtime = state.runtime
        let messages = state.messages

        emit(threadID: threadID, kind: .runStarted)
        let poller = Task { await pollRuntimeEvents(threadID: threadID, runtime: runtime) }

        do {
            let response = try await runtime.sendMessage(content, messages: messages, session: session)
            poller.cancel()
            if let error = response.error {
                emit(threadID: threadID, kind: .runFailed(error: error))
                return
            }
            appendMessage(threadID: threadID, role: .agent, content: response.content)
            emit(threadID: threadID, kind: .agentMessageCommitted(messageID: UUID().uuidString, seq: threads[threadID]?.messages.count ?? 0))
            emit(threadID: threadID, kind: .runFinished)
        } catch {
            poller.cancel()
            emit(threadID: threadID, kind: .runFailed(error: "Local operator failed."))
        }
    }

    private func pollRuntimeEvents(threadID: String, runtime: any LocalOperatorThreadRuntime) async {
        while !Task.isCancelled {
            if let event = await runtime.nextEvent(timeoutMs: 250) {
                emit(threadID: threadID, event: event)
            }
        }
    }

    private func emit(threadID: String, event: LocalOperatorRuntimeEvent) {
        switch event.kind {
        case let .agentDelta(content):
            emit(threadID: threadID, kind: .agentDelta(content: content))
        case let .toolStarted(name):
            emit(threadID: threadID, kind: .toolStarted(name: name))
        case let .toolFinished(name):
            emit(threadID: threadID, kind: .toolFinished(name: name))
        case let .waitingForApproval(id, message):
            emit(threadID: threadID, kind: .waitingForApproval(approvalID: id, message: message))
        case let .approvalResolved(id, approved):
            emit(threadID: threadID, kind: .approvalResolved(approvalID: id, approved: approved))
        case let .runFailed(error):
            emit(threadID: threadID, kind: .runFailed(error: error))
        }
    }

    private func emit(threadID: String, kind: ThreadEvent.Kind) {
        guard var state = threads[threadID] else { return }
        state.eventSeq += 1
        switch kind {
        case .runStarted, .agentDelta, .toolStarted, .waitingForApproval:
            state.running = true
        case .runFinished, .runFailed, .runCancelled:
            state.running = false
        default:
            break
        }
        if case let .waitingForApproval(approvalID, message) = kind {
            state.pendingApproval = .init(id: approvalID, message: message)
        }
        if case let .approvalResolved(approvalID, _) = kind,
           state.pendingApproval?.id == approvalID {
            state.pendingApproval = nil
        }
        if case .toolFinished = kind {
            state.pendingApproval = nil
        }
        threads[threadID] = state

        let event = ThreadEvent(seq: state.eventSeq, threadID: threadID, runID: nil, kind: kind)
        if let continuations = subscribers[threadID]?.values {
            for continuation in continuations {
                continuation.yield(event)
            }
        }
    }

    private func addSubscriber(
        _ id: UUID,
        threadID: String,
        continuation: AsyncThrowingStream<ThreadEvent, Error>.Continuation
    ) {
        subscribers[threadID, default: [:]][id] = continuation
        if let snapshot = snapshot(threadID: threadID) {
            continuation.yield(snapshot)
        }
    }

    private func removeSubscriber(_ id: UUID, threadID: String) {
        subscribers[threadID]?[id] = nil
    }

    private func snapshot(threadID: String) -> ThreadEvent? {
        guard let state = threads[threadID] else { return nil }
        return ThreadEvent(
            seq: state.eventSeq,
            threadID: threadID,
            runID: nil,
            kind: .snapshot(
                running: state.running,
                inProgress: nil,
                pendingApproval: state.pendingApproval.map {
                    ThreadEvent.PendingApproval(approvalID: $0.id, message: $0.message)
                },
                pendingQuiz: nil
            )
        )
    }

    private func appendMessage(threadID: String, role: MessageRole, content: String) {
        guard var state = threads[threadID] else { return }
        state.messages.append(makeMessage(seq: state.messages.count, role: role, content: content))
        threads[threadID] = state
    }

    private func makeMessage(seq: Int, role: MessageRole, content: String) -> Message {
        Message(
            id: UUID().uuidString,
            seq: seq,
            role: role,
            content: content,
            thinking: nil,
            toolCallName: nil
        )
    }
}
