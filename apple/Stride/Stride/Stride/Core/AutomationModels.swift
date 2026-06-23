import Foundation

/// What an automation runs when it fires. Mirrors the server `AutomationKind`.
enum AutomationKind: String, Codable, Equatable, Sendable, CaseIterable {
    case agent
    case python

    var label: String {
        switch self {
        case .agent: return "Agent prompt"
        case .python: return "Python script"
        }
    }
}

/// What makes an automation fire. Mirrors the server `TriggerKind` strings.
enum TriggerKind: String, Codable, Equatable, Sendable, CaseIterable {
    case cron
    case email
    case webhook
    case manual
    case vfsChange = "vfs_change"

    var label: String {
        switch self {
        case .cron: return "Cron schedule"
        case .email: return "Incoming email"
        case .webhook: return "Webhook (HTTP)"
        case .manual: return "Manual only"
        case .vfsChange: return "File change"
        }
    }

    var icon: String {
        switch self {
        case .cron: return "clock"
        case .email: return "envelope"
        case .webhook: return "bolt.horizontal"
        case .manual: return "hand.tap"
        case .vfsChange: return "folder"
        }
    }
}

/// Where the result of a run is delivered. Mirrors the server `NotifyKind`.
enum NotifyKind: String, Codable, Equatable, Sendable, CaseIterable {
    case none
    case telegram

    var label: String {
        switch self {
        case .none: return "Store output only"
        case .telegram: return "Telegram"
        }
    }
}

/// Lifecycle of a single execution. Mirrors the server `RunStatus`.
enum RunStatus: String, Equatable, Sendable {
    case running
    case success
    case failed

    var label: String {
        switch self {
        case .running: return "Running"
        case .success: return "Success"
        case .failed: return "Failed"
        }
    }
}

/// One automation. Mirrors `GET /api/automations`. Enum-typed fields are stored
/// as raw strings and exposed through computed accessors so an unrecognised
/// server value never breaks decoding of the whole list.
struct Automation: Identifiable, Equatable, Decodable, Sendable {
    let id: String
    let name: String
    let schedule: String
    let kind: String
    let payload: String
    let enabled: Bool
    let createdAt: Int64
    let lastRun: Int64?
    let triggerKind: String
    let notifyKind: String
    /// Account id pulled out of `trigger_config` for the email trigger.
    let emailAccountID: String?
    /// Present only in the response to creating a webhook automation.
    let webhookSecret: String?

    enum CodingKeys: String, CodingKey {
        case id, name, schedule, kind, payload, enabled
        case createdAt = "created_at"
        case lastRun = "last_run"
        case triggerKind = "trigger_kind"
        case notifyKind = "notify_kind"
        case triggerConfig = "trigger_config"
        case webhookSecret = "webhook_secret"
    }

    private struct TriggerConfig: Decodable {
        let accountID: String?
        enum CodingKeys: String, CodingKey { case accountID = "account_id" }
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        id = try container.decode(String.self, forKey: .id)
        name = try container.decode(String.self, forKey: .name)
        schedule = try container.decodeIfPresent(String.self, forKey: .schedule) ?? ""
        kind = try container.decodeIfPresent(String.self, forKey: .kind) ?? AutomationKind.agent.rawValue
        payload = try container.decodeIfPresent(String.self, forKey: .payload) ?? ""
        enabled = try container.decodeIfPresent(Bool.self, forKey: .enabled) ?? false
        createdAt = try container.decodeIfPresent(Int64.self, forKey: .createdAt) ?? 0
        lastRun = try container.decodeIfPresent(Int64.self, forKey: .lastRun)
        triggerKind = try container.decodeIfPresent(String.self, forKey: .triggerKind) ?? TriggerKind.cron.rawValue
        notifyKind = try container.decodeIfPresent(String.self, forKey: .notifyKind) ?? NotifyKind.none.rawValue
        webhookSecret = try container.decodeIfPresent(String.self, forKey: .webhookSecret)
        // trigger_config is free-form JSON; tolerate any shape and keep only the
        // email account id we display.
        if let config = try? container.decodeIfPresent(TriggerConfig.self, forKey: .triggerConfig) {
            emailAccountID = config.accountID
        } else {
            emailAccountID = nil
        }
    }
}

extension Automation {
    var kindValue: AutomationKind { AutomationKind(rawValue: kind) ?? .agent }
    var triggerValue: TriggerKind { TriggerKind(rawValue: triggerKind) ?? .cron }
    var notifyValue: NotifyKind { NotifyKind(rawValue: notifyKind) ?? .none }

    var createdDate: Date { Date(timeIntervalSince1970: TimeInterval(createdAt)) }
    var lastRunDate: Date? { lastRun.map { Date(timeIntervalSince1970: TimeInterval($0)) } }

    var lastRunLabel: String {
        guard let date = lastRunDate else { return "Never run" }
        return "Last run \(date.formatted(date: .abbreviated, time: .shortened))"
    }

    /// Short human description of when the automation fires, resolving an email
    /// account name when one is configured.
    func triggerDescription(accounts: [EmailAccount]) -> String {
        switch triggerValue {
        case .cron:
            return schedule.isEmpty ? "Cron schedule" : schedule
        case .email:
            if let id = emailAccountID, let account = accounts.first(where: { $0.id == id }) {
                return "New mail in \(account.name)"
            }
            return "New incoming email"
        case .webhook:
            return "Webhook"
        case .manual:
            return "Manual"
        case .vfsChange:
            return "File change"
        }
    }
}

/// One execution of an automation. Mirrors `GET /api/automations/{id}/runs`.
struct AutomationRun: Identifiable, Equatable, Decodable, Sendable {
    let id: String
    let startedAt: Int64
    let finishedAt: Int64?
    let status: String
    let output: String

    enum CodingKeys: String, CodingKey {
        case id
        case startedAt = "started_at"
        case finishedAt = "finished_at"
        case status, output
    }
}

extension AutomationRun {
    var statusValue: RunStatus { RunStatus(rawValue: status) ?? .running }
    var isFinished: Bool { finishedAt != nil }

    var startedDate: Date { Date(timeIntervalSince1970: TimeInterval(startedAt)) }
    var finishedDate: Date? { finishedAt.map { Date(timeIntervalSince1970: TimeInterval($0)) } }

    var startedLabel: String { startedDate.formatted(date: .abbreviated, time: .shortened) }
    var finishedLabel: String {
        guard let date = finishedDate else { return "Still running" }
        return date.formatted(date: .abbreviated, time: .shortened)
    }
}

/// The payload for `POST /api/automations`. Encoded to the server's snake_case
/// body in `StrideClientLive`.
struct NewAutomation: Equatable, Sendable {
    var name: String
    var schedule: String
    var kind: AutomationKind
    var payload: String
    var enabled: Bool
    var triggerKind: TriggerKind
    var notifyKind: NotifyKind
    /// For the vfs_change trigger `{ "path": ... }`, for email `{ "account_id": ... }`.
    var triggerConfig: [String: String]?
}

/// A configured IMAP inbox, used to populate the email trigger picker. Mirrors
/// `GET /api/settings/email`; extra fields are ignored.
struct EmailAccount: Identifiable, Equatable, Decodable, Sendable {
    let id: String
    let name: String
    let email: String
}
