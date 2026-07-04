import Foundation

/// A conversation grouping. Mirrors the server `projects` table.
struct Project: Identifiable, Equatable, Decodable {
    let id: String
    let title: String
}

enum ThreadLocation: String, CaseIterable, Equatable, Hashable, Sendable {
    case local
    case cloud

    var label: String {
        switch self {
        case .local: return "Local"
        case .cloud: return "Cloud"
        }
    }
}

/// One row in the thread list. Mirrors `GET /api/threads`.
struct ThreadSummary: Identifiable, Equatable, Decodable {
    let id: String
    var title: String
    var projectID: String?
    var location: ThreadLocation = .cloud

    enum CodingKeys: String, CodingKey {
        case id, title
        case projectID = "project_id"
    }
}

/// Roles a stored message can carry.
enum MessageRole: String, Equatable, Decodable {
    case system, agent, user, tool
}

/// A persisted message. Mirrors `GET /api/threads/{id}/messages`.
struct Message: Identifiable, Equatable, Decodable {
    let id: String
    let seq: Int
    let role: MessageRole
    let content: String
    let thinking: String?
    let toolCallName: String?

    enum CodingKeys: String, CodingKey {
        case id, seq, role, content, thinking
        case toolCallName = "tool_call_name"
    }
}

/// Response from creating a thread or sending a message.
struct SendResult: Equatable, Decodable {
    let threadID: String
    let runID: String

    enum CodingKeys: String, CodingKey {
        case threadID = "thread_id"
        case runID = "run_id"
    }
}

/// A multiple-choice question the agent asks mid-run.
struct QuizQuestion: Equatable, Decodable {
    let question: String
    let options: [String]
}
