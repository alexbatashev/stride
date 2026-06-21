import ComposableArchitecture
import Foundation

@Reducer
struct AuthFeature {
    @ObservableState
    struct State: Equatable {
        var mode: Mode = .login
        var serverURL: String = Session.shared.baseURL?.absoluteString ?? ""
        var username = ""
        var password = ""
        var isSubmitting = false
        var errorMessage: String?

        enum Mode: Equatable {
            case login
            case register
        }

        var canSubmit: Bool {
            !serverURL.trimmingCharacters(in: .whitespaces).isEmpty
                && !username.trimmingCharacters(in: .whitespaces).isEmpty
                && !password.isEmpty
                && !isSubmitting
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case submitTapped
        case authResponse(Result<Void, FridayError>)
        case delegate(Delegate)

        enum Delegate: Equatable {
            case authenticated
        }
    }

    @Dependency(\.friday) var friday

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding(\.mode):
                state.errorMessage = nil
                return .none

            case .binding:
                return .none

            case .submitTapped:
                guard let url = normalizedURL(state.serverURL) else {
                    state.errorMessage = "Enter a valid server URL."
                    return .none
                }
                guard state.canSubmit else { return .none }

                state.isSubmitting = true
                state.errorMessage = nil
                let mode = state.mode
                let username = state.username.trimmingCharacters(in: .whitespaces)
                let password = state.password

                return .run { send in
                    do {
                        if mode == .login {
                            try await friday.login(url, username, password)
                        } else {
                            try await friday.register(url, username, password)
                        }
                        await send(.authResponse(.success(())))
                    } catch let error as FridayError {
                        await send(.authResponse(.failure(error)))
                    } catch {
                        await send(.authResponse(.failure(.transport)))
                    }
                }

            case .authResponse(.success):
                state.isSubmitting = false
                return .send(.delegate(.authenticated))

            case let .authResponse(.failure(error)):
                state.isSubmitting = false
                state.errorMessage = message(for: error, mode: state.mode)
                return .none

            case .delegate:
                return .none
            }
        }
    }

    private func message(for error: FridayError, mode: State.Mode) -> String {
        switch error {
        case .notConfigured:
            return "Enter a valid server URL."
        case .unauthorized:
            return "Username or password is incorrect."
        case .http(403):
            return mode == .register ? "Registration is disabled on this server." : "Could not sign in."
        case .http(409):
            return mode == .register ? "That username is already taken." : "Could not sign in."
        case .http(400):
            return "Enter a username and password."
        case .http, .transport:
            return "Could not reach the server. Check the URL and try again."
        }
    }
}

/// Accepts bare hosts ("friday.example.com") and full URLs, defaulting to https.
private func normalizedURL(_ raw: String) -> URL? {
    var trimmed = raw.trimmingCharacters(in: .whitespaces)
    guard !trimmed.isEmpty else { return nil }
    if !trimmed.contains("://") {
        trimmed = "https://" + trimmed
    }
    guard let url = URL(string: trimmed), url.host != nil else { return nil }
    return url
}
