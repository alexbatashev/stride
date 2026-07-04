import Foundation

/// A single frame from the thread event WebSocket (`GET /api/threads/{id}/events`).
/// The server tags each `kind` with a `type` discriminator; this type decodes the
/// envelope plus the tagged payload into a Swift enum.
struct ThreadEvent: Equatable, Decodable {
    let seq: Int
    let threadID: String
    let runID: String?
    let kind: Kind

    struct InProgress: Equatable, Decodable {
        let runID: String
        let content: String
        let thinking: String?

        enum CodingKeys: String, CodingKey {
            case runID = "run_id"
            case content, thinking
        }
    }

    struct PendingApproval: Equatable, Decodable {
        let approvalID: String
        let message: String

        enum CodingKeys: String, CodingKey {
            case approvalID = "approval_id"
            case message
        }
    }

    struct PendingQuiz: Equatable, Decodable {
        let quizID: String
        let questions: [QuizQuestion]

        enum CodingKeys: String, CodingKey {
            case quizID = "quiz_id"
            case questions
        }
    }

    enum Kind: Equatable {
        case snapshot(
            running: Bool,
            inProgress: InProgress?,
            pendingApproval: PendingApproval?,
            pendingQuiz: PendingQuiz?
        )
        case runStarted
        case userMessageCommitted(messageID: String, seq: Int)
        case agentDelta(content: String)
        case thinkingDelta(thinking: String)
        case agentMessageCommitted(messageID: String, seq: Int)
        case toolStarted(name: String)
        case toolFinished(name: String)
        case waitingForApproval(approvalID: String, message: String)
        case approvalResolved(approvalID: String, approved: Bool)
        case waitingForQuiz(quizID: String, questions: [QuizQuestion])
        case quizAnswered(quizID: String)
        case runFinished
        case runFailed(error: String)
        case runCancelled
        case unknown
    }

    enum CodingKeys: String, CodingKey {
        case seq
        case threadID = "thread_id"
        case runID = "run_id"
        case kind
    }

    init(seq: Int, threadID: String, runID: String?, kind: Kind) {
        self.seq = seq
        self.threadID = threadID
        self.runID = runID
        self.kind = kind
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        seq = try container.decode(Int.self, forKey: .seq)
        threadID = try container.decode(String.self, forKey: .threadID)
        runID = try container.decodeIfPresent(String.self, forKey: .runID)
        kind = try Kind(from: container.superDecoder(forKey: .kind))
    }
}

extension ThreadEvent.Kind: Decodable {
    private enum CodingKeys: String, CodingKey {
        case type, status
        case inProgress = "in_progress"
        case pendingApproval = "pending_approval"
        case pendingQuiz = "pending_quiz"
        case messageID = "message_id"
        case seq, content, thinking, name
        case approvalID = "approval_id"
        case message, approved
        case quizID = "quiz_id"
        case questions, error
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        let type = try container.decode(String.self, forKey: .type)

        switch type {
        case "Snapshot":
            let status = try container.decodeIfPresent(String.self, forKey: .status)
            self = .snapshot(
                running: status == "running",
                inProgress: try container.decodeIfPresent(ThreadEvent.InProgress.self, forKey: .inProgress),
                pendingApproval: try container.decodeIfPresent(ThreadEvent.PendingApproval.self, forKey: .pendingApproval),
                pendingQuiz: try container.decodeIfPresent(ThreadEvent.PendingQuiz.self, forKey: .pendingQuiz)
            )
        case "RunStarted":
            self = .runStarted
        case "UserMessageCommitted":
            self = .userMessageCommitted(
                messageID: try container.decode(String.self, forKey: .messageID),
                seq: try container.decode(Int.self, forKey: .seq)
            )
        case "AgentDelta":
            self = .agentDelta(content: try container.decode(String.self, forKey: .content))
        case "ThinkingDelta":
            self = .thinkingDelta(thinking: try container.decode(String.self, forKey: .thinking))
        case "AgentMessageCommitted":
            self = .agentMessageCommitted(
                messageID: try container.decode(String.self, forKey: .messageID),
                seq: try container.decode(Int.self, forKey: .seq)
            )
        case "ToolStarted":
            self = .toolStarted(name: try container.decode(String.self, forKey: .name))
        case "ToolFinished":
            self = .toolFinished(name: try container.decode(String.self, forKey: .name))
        case "WaitingForApproval":
            self = .waitingForApproval(
                approvalID: try container.decode(String.self, forKey: .approvalID),
                message: try container.decode(String.self, forKey: .message)
            )
        case "ApprovalResolved":
            self = .approvalResolved(
                approvalID: try container.decode(String.self, forKey: .approvalID),
                approved: try container.decode(Bool.self, forKey: .approved)
            )
        case "WaitingForQuiz":
            self = .waitingForQuiz(
                quizID: try container.decode(String.self, forKey: .quizID),
                questions: try container.decode([QuizQuestion].self, forKey: .questions)
            )
        case "QuizAnswered":
            self = .quizAnswered(quizID: try container.decode(String.self, forKey: .quizID))
        case "RunFinished":
            self = .runFinished
        case "RunFailed":
            self = .runFailed(error: try container.decode(String.self, forKey: .error))
        case "RunCancelled":
            self = .runCancelled
        default:
            self = .unknown
        }
    }
}
