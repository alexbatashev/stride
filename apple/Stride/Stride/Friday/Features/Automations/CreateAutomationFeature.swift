import ComposableArchitecture
import Foundation

/// The "new automation" form. Owns its fields and emits a `NewAutomation` to the
/// parent on submit; the parent performs the create call.
@Reducer
struct CreateAutomationFeature {
    @ObservableState
    struct State: Equatable {
        var name = ""
        var triggerKind: TriggerKind = .cron
        var schedule = ""
        var emailAccountID = ""
        var watchPath = ""
        var kind: AutomationKind = .agent
        var notifyKind: NotifyKind = .none
        var payload = ""
        var enabled = true

        /// True while the parent is performing the create request.
        var submitting = false
        var errorMessage: String?

        /// Inboxes offered for the email trigger, supplied by the parent.
        var emailAccounts: IdentifiedArrayOf<EmailAccount> = []

        var canSubmit: Bool {
            guard !name.trimmed.isEmpty, !payload.trimmed.isEmpty else { return false }
            switch triggerKind {
            case .cron: return !schedule.trimmed.isEmpty
            case .email: return !emailAccountID.isEmpty
            default: return true
            }
        }

        var payloadPrompt: String {
            kind == .python
                ? "Paste a Python script…"
                : "Describe the task for the agent…"
        }

        /// Builds the request body from the current field values.
        var draft: NewAutomation {
            var config: [String: String]?
            switch triggerKind {
            case .vfsChange:
                config = ["path": watchPath.trimmed]
            case .email:
                config = ["account_id": emailAccountID]
            default:
                config = nil
            }
            return NewAutomation(
                name: name.trimmed,
                schedule: triggerKind == .cron ? schedule.trimmed : "",
                kind: kind,
                payload: payload,
                enabled: enabled,
                triggerKind: triggerKind,
                notifyKind: notifyKind,
                triggerConfig: config
            )
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case cancelTapped
        case submitTapped
        case submitFailed(String)
        case delegate(Delegate)

        enum Delegate: Equatable {
            case cancel
            case submit(NewAutomation)
        }
    }

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding:
                return .none

            case .cancelTapped:
                return .send(.delegate(.cancel))

            case .submitTapped:
                guard state.canSubmit, !state.submitting else { return .none }
                state.submitting = true
                state.errorMessage = nil
                return .send(.delegate(.submit(state.draft)))

            case let .submitFailed(message):
                state.submitting = false
                state.errorMessage = message
                return .none

            case .delegate:
                return .none
            }
        }
    }
}

extension String {
    /// Whitespace- and newline-trimmed copy.
    var trimmed: String { trimmingCharacters(in: .whitespacesAndNewlines) }
}
