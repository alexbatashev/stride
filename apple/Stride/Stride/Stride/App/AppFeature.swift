import ComposableArchitecture

/// Root reducer. Shows the auth screen until a session exists, then the main
/// three-column home. Logout tears the home back down.
@Reducer
struct AppFeature {
    @ObservableState
    struct State: Equatable {
        var auth = AuthFeature.State()
        var home: HomeFeature.State? = Session.shared.isAuthenticated ? HomeFeature.State() : nil
    }

    enum Action {
        case auth(AuthFeature.Action)
        case home(HomeFeature.Action)
    }

    var body: some ReducerOf<Self> {
        Scope(state: \.auth, action: \.auth) {
            AuthFeature()
        }
        Reduce { state, action in
            switch action {
            case .auth(.delegate(.authenticated)):
                state.home = HomeFeature.State()
                state.auth = AuthFeature.State()
                return .none

            case .auth:
                return .none

            case .home(.delegate(.loggedOut)):
                state.home = nil
                state.auth = AuthFeature.State()
                return .none

            case .home:
                return .none
            }
        }
        .ifLet(\.home, action: \.home) {
            HomeFeature()
        }
    }
}
