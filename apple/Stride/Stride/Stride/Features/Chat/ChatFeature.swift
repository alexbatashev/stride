import ComposableArchitecture
import Foundation

/// A message as shown in the conversation. Streaming text lives separately in
/// ``ChatFeature/State/streaming`` until the server commits it.
struct ChatMessage: Identifiable, Equatable {
    let id: String
    var seq: Int
    var role: MessageRole
    var content: String
    var thinking: String?
    var toolName: String?
    var pending: Bool = false
}

@Reducer
struct ChatFeature {
    @ObservableState
    struct State: Equatable, Identifiable {
        /// Stable for the lifetime of one conversation instance, so creating a
        /// thread (nil → real id) does not rebuild the detail view. A `let` with
        /// an initializer evaluates per instance and is excluded from the
        /// synthesized memberwise init.
        let draftID = UUID()
        var threadID: String?
        var projectID: String?
        var title: String = "New thread"
        var messages: IdentifiedArrayOf<ChatMessage> = []
        var streaming: Streaming?
        var running = false
        var activeTool: String?
        var pendingApproval: Approval?
        var pendingQuiz: Quiz?
        var composer = ""
        var errorMessage: String?
        var isLoadingHistory = false
        var lastEventSeq = -1
        var files: FilesFeature.State?
        var showFiles = false

        var id: UUID { draftID }
        var isNewThread: Bool { threadID == nil }
        var trimmedComposer: String { composer.trimmingCharacters(in: .whitespacesAndNewlines) }
        var canSend: Bool { !trimmedComposer.isEmpty && !running && pendingApproval == nil && pendingQuiz == nil }

        struct Streaming: Equatable {
            var content = ""
            var thinking = ""
        }

        struct Approval: Equatable {
            let id: String
            let message: String
        }

        struct Quiz: Equatable {
            let id: String
            let questions: [QuizQuestion]
            var index = 0
            var answers: [String] = []
            var current: QuizQuestion? { index < questions.count ? questions[index] : nil }
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case onAppear
        case connect
        case event(ThreadEvent)
        case reloadHistory
        case historyResponse(Result<[Message], StrideError>)
        case sendTapped
        case threadCreated(Result<SendResult, StrideError>)
        case sendFailed(StrideError)
        case cancelTapped
        case approvalResponse(Bool)
        case approvalFailed(State.Approval)
        case quizSelected(String)
        case quizFailed(State.Quiz)
        case dismissError
        case filesButtonTapped
        case files(FilesFeature.Action)
        case delegate(Delegate)

        enum Delegate: Equatable {
            case threadCreated(id: String, projectID: String?)
            case threadsNeedRefresh
        }
    }

    private enum CancelID { case events, history }

    @Dependency(\.stride) var stride
    @Dependency(\.continuousClock) var clock

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding:
                return .none

            case .onAppear:
                guard state.threadID != nil else { return .none }
                state.isLoadingHistory = state.messages.isEmpty
                return .merge(.send(.reloadHistory), .send(.connect))

            case .connect:
                guard let threadID = state.threadID else { return .none }
                return .run { send in
                    while !Task.isCancelled {
                        do {
                            for try await event in stride.events(threadID) {
                                await send(.event(event))
                            }
                        } catch {}
                        if Task.isCancelled { break }
                        try? await clock.sleep(for: .seconds(2))
                    }
                }
                .cancellable(id: CancelID.events, cancelInFlight: true)

            case .reloadHistory:
                guard let threadID = state.threadID else { return .none }
                return .run { send in
                    await send(.historyResponse(loadMessages(threadID)))
                }
                .cancellable(id: CancelID.history, cancelInFlight: true)

            case let .historyResponse(.success(messages)):
                state.isLoadingHistory = false
                state.messages = IdentifiedArray(uniqueElements: messages.map(chatMessage))
                state.streaming = nil
                return .none

            case .historyResponse(.failure):
                state.isLoadingHistory = false
                return .none

            case let .event(event):
                return reduceEvent(&state, event)

            case .sendTapped:
                let content = state.trimmedComposer
                guard !content.isEmpty, !state.running else { return .none }
                state.composer = ""
                state.errorMessage = nil
                state.running = true
                appendPendingUser(&state, content: content)

                if let threadID = state.threadID {
                    return .run { send in
                        do {
                            _ = try await stride.sendMessage(threadID, content, [])
                        } catch let error as StrideError {
                            await send(.sendFailed(error))
                        } catch {
                            await send(.sendFailed(.transport))
                        }
                    }
                } else {
                    let projectID = state.projectID
                    return .run { send in
                        do {
                            let result = try await stride.createThread(content, projectID, [])
                            await send(.threadCreated(.success(result)))
                        } catch let error as StrideError {
                            await send(.threadCreated(.failure(error)))
                        } catch {
                            await send(.threadCreated(.failure(.transport)))
                        }
                    }
                }

            case let .threadCreated(.success(result)):
                state.threadID = result.threadID
                state.lastEventSeq = -1
                state.files = nil
                return .merge(
                    .send(.delegate(.threadCreated(id: result.threadID, projectID: state.projectID))),
                    .send(.connect),
                    .send(.reloadHistory)
                )

            case .threadCreated(.failure):
                return failSend(&state)

            case .sendFailed:
                return failSend(&state)

            case .cancelTapped:
                guard let threadID = state.threadID, state.running else { return .none }
                return .run { _ in try? await stride.cancelRun(threadID) }

            case let .approvalResponse(approved):
                guard let threadID = state.threadID, let approval = state.pendingApproval else { return .none }
                state.pendingApproval = nil
                return .run { send in
                    do {
                        try await stride.resolveApproval(threadID, approval.id, approved)
                    } catch {
                        await send(.approvalFailed(approval))
                    }
                }

            case let .approvalFailed(approval):
                state.pendingApproval = approval
                state.errorMessage = "Couldn't send your response. Try again."
                return .none

            case let .quizSelected(answer):
                guard let threadID = state.threadID, var quiz = state.pendingQuiz else { return .none }
                quiz.answers.append(answer)
                if quiz.index + 1 < quiz.questions.count {
                    quiz.index += 1
                    state.pendingQuiz = quiz
                    return .none
                }
                state.pendingQuiz = nil
                let answers = quiz.answers
                let quizID = quiz.id
                let completedQuiz = quiz
                return .run { send in
                    do {
                        try await stride.answerQuiz(threadID, quizID, answers)
                    } catch {
                        await send(.quizFailed(completedQuiz))
                    }
                }

            case let .quizFailed(quiz):
                state.pendingQuiz = quiz
                state.errorMessage = "Couldn't send your answers. Try again."
                return .none

            case .dismissError:
                state.errorMessage = nil
                return .none

            case .filesButtonTapped:
                guard let threadID = state.threadID else { return .none }
                if state.files == nil {
                    state.files = FilesFeature.State(scope: .workspace(threadID: threadID))
                }
                state.showFiles.toggle()
                return .none

            case .files:
                return .none

            case .delegate:
                return .none
            }
        }
        .ifLet(\.files, action: \.files) {
            FilesFeature()
        }
    }

    // MARK: - Event handling

    private func reduceEvent(_ state: inout State, _ event: ThreadEvent) -> Effect<Action> {
        guard event.threadID == state.threadID else { return .none }

        if case .snapshot = event.kind {
            state.lastEventSeq = event.seq
        } else {
            guard event.seq > state.lastEventSeq else { return .none }
            state.lastEventSeq = event.seq
        }

        switch event.kind {
        case let .snapshot(running, inProgress, pendingApproval, pendingQuiz):
            state.running = running
            state.pendingApproval = pendingApproval.map { .init(id: $0.approvalID, message: $0.message) }
            state.pendingQuiz = pendingQuiz.flatMap(makeQuiz)
            if let inProgress, !inProgress.content.isEmpty {
                state.streaming = .init(content: inProgress.content, thinking: inProgress.thinking ?? "")
            }
            return .none

        case .runStarted:
            state.running = true
            return .none

        case .userMessageCommitted:
            // The authoritative message arrives on the next history reload, which
            // replaces the optimistic bubble; nothing to do here.
            return .none

        case let .agentDelta(content):
            state.running = true
            state.streaming = state.streaming ?? .init()
            state.streaming?.content += content
            return .none

        case let .thinkingDelta(thinking):
            state.streaming = state.streaming ?? .init()
            state.streaming?.thinking += thinking
            return .none

        case .agentMessageCommitted:
            state.streaming = nil
            return .send(.reloadHistory)

        case let .toolStarted(name):
            state.running = true
            state.activeTool = name
            return .none

        case .toolFinished:
            state.activeTool = nil
            state.pendingApproval = nil
            state.pendingQuiz = nil
            return .send(.reloadHistory)

        case let .waitingForApproval(approvalID, message):
            state.running = true
            state.pendingApproval = .init(id: approvalID, message: message)
            return .none

        case let .approvalResolved(approvalID, _):
            if state.pendingApproval?.id == approvalID { state.pendingApproval = nil }
            return .none

        case let .waitingForQuiz(quizID, questions):
            state.running = true
            state.pendingQuiz = .init(id: quizID, questions: questions)
            return .none

        case let .quizAnswered(quizID):
            if state.pendingQuiz?.id == quizID { state.pendingQuiz = nil }
            return .none

        case .runFinished:
            resetRun(&state)
            return .merge(.send(.reloadHistory), .send(.delegate(.threadsNeedRefresh)))

        case let .runFailed(error):
            resetRun(&state)
            state.errorMessage = error
            return .none

        case .runCancelled:
            resetRun(&state)
            return .send(.reloadHistory)

        case .unknown:
            return .none
        }
    }

    private func resetRun(_ state: inout State) {
        state.running = false
        state.activeTool = nil
        state.pendingApproval = nil
        state.pendingQuiz = nil
        state.streaming = nil
    }

    private func failSend(_ state: inout State) -> Effect<Action> {
        state.running = false
        state.messages.removeAll { $0.pending && $0.role == .user }
        state.errorMessage = "Couldn't send your message."
        return .none
    }

    private func appendPendingUser(_ state: inout State, content: String) {
        let message = ChatMessage(
            id: "pending-user-\(state.draftID.uuidString)-\(state.messages.count)",
            seq: Int.max,
            role: .user,
            content: content,
            thinking: nil,
            toolName: nil,
            pending: true
        )
        state.messages.append(message)
    }

    private func chatMessage(_ message: Message) -> ChatMessage {
        ChatMessage(
            id: message.id,
            seq: message.seq,
            role: message.role,
            content: message.content,
            thinking: message.thinking,
            toolName: message.toolCallName
        )
    }

    private func makeQuiz(_ quiz: ThreadEvent.PendingQuiz) -> State.Quiz? {
        quiz.questions.isEmpty ? nil : .init(id: quiz.quizID, questions: quiz.questions)
    }

    private func loadMessages(_ threadID: String) async -> Result<[Message], StrideError> {
        do {
            return .success(try await stride.listMessages(threadID))
        } catch let error as StrideError {
            return .failure(error)
        } catch {
            return .failure(.transport)
        }
    }
}
