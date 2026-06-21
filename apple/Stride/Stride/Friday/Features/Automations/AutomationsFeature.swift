import ComposableArchitecture
import Foundation

/// Manages the automation list (content column) and the run history of the
/// selected automation (detail column): loading, creating, enabling, deleting,
/// running on demand and polling for the resulting output.
@Reducer
struct AutomationsFeature {
    @ObservableState
    struct State: Equatable {
        var automations: IdentifiedArrayOf<Automation> = []
        var emailAccounts: IdentifiedArrayOf<EmailAccount> = []
        var selectedID: Automation.ID?

        var runs: IdentifiedArrayOf<AutomationRun> = []
        var isLoading = false
        var isLoadingRuns = false
        var errorMessage: String?

        /// Automations with an in-flight manual run (drives the row spinner).
        var runningIDs: Set<Automation.ID> = []

        @Presents var create: CreateAutomationFeature.State?
        var webhookReveal: WebhookReveal?
        var deleteTarget: Automation?

        var selectedAutomation: Automation? {
            guard let selectedID else { return nil }
            return automations[id: selectedID]
        }

        var hasRunningSelected: Bool {
            guard let selectedID else { return false }
            return runningIDs.contains(selectedID)
        }

        /// The webhook URL and secret shown once after creation.
        struct WebhookReveal: Equatable, Identifiable {
            let id: String
            let url: String
            let secret: String
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case onAppear
        case refresh
        case load
        case loaded(Result<[Automation], FridayError>)
        case emailAccountsLoaded([EmailAccount])

        case automationSelected(Automation.ID?)
        case loadRuns(Automation.ID)
        case runsLoaded(id: Automation.ID, Result<[AutomationRun], FridayError>)

        case runTapped(Automation.ID)
        case runsPolled(id: Automation.ID, [AutomationRun])
        case runFinished(Automation.ID)

        case toggleEnabled(Automation)
        case enabledUpdated(Result<Void, FridayError>)

        case deleteTapped(Automation)
        case confirmDelete
        case deleteFinished(Result<Void, FridayError>)

        case newTapped
        case create(PresentationAction<CreateAutomationFeature.Action>)
        case createFinished(Result<Automation, FridayError>)

        case setError(String?)
        case dismissError
        case delegate(Delegate)

        enum Delegate: Equatable {
            /// Emitted when an automation is selected so the parent can reveal the
            /// detail column on compact layouts.
            case automationSelected
        }
    }

    @Dependency(\.friday) var friday
    @Dependency(\.continuousClock) var clock

    private enum CancelID: Hashable {
        case load
        case runs
        case run(Automation.ID)
    }

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding:
                return .none

            case .onAppear:
                state.isLoading = state.automations.isEmpty
                return .merge(.send(.load), loadEmailAccounts())

            case .refresh:
                return .send(.load)

            case .load:
                return .run { send in
                    await send(.loaded(await asyncResult { try await friday.listAutomations() }))
                }
                .cancellable(id: CancelID.load, cancelInFlight: true)

            case let .loaded(.success(automations)):
                state.isLoading = false
                state.errorMessage = nil
                state.automations = IdentifiedArray(uniqueElements: automations)
                if let selectedID = state.selectedID, state.automations[id: selectedID] == nil {
                    state.selectedID = nil
                    state.runs = []
                }
                return .none

            case let .loaded(.failure(error)):
                state.isLoading = false
                state.errorMessage = message(for: error)
                return .none

            case let .emailAccountsLoaded(accounts):
                state.emailAccounts = IdentifiedArray(uniqueElements: accounts)
                return .none

            case let .automationSelected(id):
                state.selectedID = id
                state.runs = []
                guard let id else { return .none }
                state.isLoadingRuns = true
                return .merge(.send(.loadRuns(id)), .send(.delegate(.automationSelected)))

            case let .loadRuns(id):
                return .run { send in
                    await send(.runsLoaded(id: id, await asyncResult { try await friday.listAutomationRuns(id) }))
                }
                .cancellable(id: CancelID.runs, cancelInFlight: true)

            case let .runsLoaded(id, .success(runs)):
                guard state.selectedID == id else { return .none }
                state.isLoadingRuns = false
                state.runs = IdentifiedArray(uniqueElements: runs)
                return .none

            case let .runsLoaded(id, .failure(error)):
                guard state.selectedID == id else { return .none }
                state.isLoadingRuns = false
                state.errorMessage = message(for: error)
                return .none

            case let .runTapped(id):
                state.runningIDs.insert(id)
                state.errorMessage = nil
                if state.selectedID != id {
                    state.selectedID = id
                    state.runs = []
                    state.isLoadingRuns = true
                }
                return .merge(
                    .send(.delegate(.automationSelected)),
                    pollRun(id: id)
                )

            case let .runsPolled(id, runs):
                guard state.selectedID == id else { return .none }
                state.isLoadingRuns = false
                state.runs = IdentifiedArray(uniqueElements: runs)
                return .none

            case let .runFinished(id):
                state.runningIDs.remove(id)
                // Reload the list so the automation's "last run" updates.
                return .send(.load)

            case let .toggleEnabled(automation):
                let id = automation.id
                let enabled = !automation.enabled
                return .run { send in
                    await send(.enabledUpdated(await asyncResult {
                        try await friday.setAutomationEnabled(id, enabled)
                    }))
                }

            case .enabledUpdated(.success):
                return .send(.load)

            case let .enabledUpdated(.failure(error)):
                state.errorMessage = message(for: error)
                return .none

            case let .deleteTapped(automation):
                state.deleteTarget = automation
                return .none

            case .confirmDelete:
                guard let target = state.deleteTarget else { return .none }
                state.deleteTarget = nil
                let id = target.id
                if state.selectedID == id {
                    state.selectedID = nil
                    state.runs = []
                }
                return .run { send in
                    await send(.deleteFinished(await asyncResult { try await friday.deleteAutomation(id) }))
                }

            case .deleteFinished(.success):
                return .send(.load)

            case let .deleteFinished(.failure(error)):
                state.errorMessage = message(for: error)
                return .none

            case .newTapped:
                state.create = CreateAutomationFeature.State(emailAccounts: state.emailAccounts)
                return .none

            case .create(.presented(.delegate(.cancel))):
                state.create = nil
                return .none

            case let .create(.presented(.delegate(.submit(draft)))):
                return .run { send in
                    await send(.createFinished(await asyncResult { try await friday.createAutomation(draft) }))
                }

            case .create:
                return .none

            case let .createFinished(.success(automation)):
                state.create = nil
                state.selectedID = automation.id
                state.runs = []
                if let secret = automation.webhookSecret {
                    state.webhookReveal = State.WebhookReveal(
                        id: automation.id,
                        url: webhookURL(for: automation.id),
                        secret: secret
                    )
                }
                return .send(.load)

            case let .createFinished(.failure(error)):
                // Keep the form open and surface the reason.
                return .send(.create(.presented(.submitFailed(createMessage(for: error)))))

            case let .setError(message):
                state.errorMessage = message
                return .none

            case .dismissError:
                state.errorMessage = nil
                return .none

            case .delegate:
                return .none
            }
        }
        .ifLet(\.$create, action: \.create) {
            CreateAutomationFeature()
        }
    }

    private func loadEmailAccounts() -> Effect<Action> {
        .run { send in
            let accounts = (try? await friday.listEmailAccounts()) ?? []
            await send(.emailAccountsLoaded(accounts))
        }
    }

    /// Fires a manual run, then polls the run history until the newest run
    /// finishes (or the attempts run out), pushing each snapshot to the UI.
    private func pollRun(id: Automation.ID) -> Effect<Action> {
        .run { send in
            do {
                try await friday.runAutomation(id)
            } catch {
                await send(.setError(message(for: FridayError.from(error))))
                await send(.runFinished(id))
                return
            }
            for attempt in 0 ..< 16 {
                try? await clock.sleep(for: .milliseconds(attempt < 4 ? 600 : 1200))
                guard let runs = try? await friday.listAutomationRuns(id) else { continue }
                await send(.runsPolled(id: id, runs))
                if let latest = runs.first, latest.isFinished {
                    break
                }
            }
            await send(.runFinished(id))
        }
        .cancellable(id: CancelID.run(id), cancelInFlight: true)
    }

    private func webhookURL(for id: Automation.ID) -> String {
        let path = "/api/automations/\(id)/webhook"
        guard let base = friday.serverBaseURL() else { return path }
        var trimmed = base.absoluteString
        if trimmed.hasSuffix("/") { trimmed.removeLast() }
        return trimmed + path
    }

    private func message(for error: FridayError) -> String {
        switch error {
        case .unauthorized: return "Your session expired. Sign in again."
        case .notConfigured: return "No server configured."
        default: return "Something went wrong. Try again."
        }
    }

    private func createMessage(for error: FridayError) -> String {
        if case .http(400) = error {
            return "Check the name, cron schedule, and task."
        }
        return "Couldn't create the automation. Try again."
    }
}

/// Runs an async throwing call and captures the outcome as a `Result`.
private func asyncResult<T>(_ body: () async throws -> T) async -> Result<T, FridayError> {
    do {
        return .success(try await body())
    } catch {
        return .failure(FridayError.from(error))
    }
}
