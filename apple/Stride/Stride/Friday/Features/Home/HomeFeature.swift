import ComposableArchitecture
import SwiftUI

@Reducer
struct HomeFeature {
    @ObservableState
    struct State: Equatable {
        var projects: IdentifiedArrayOf<Project> = []
        var threads: IdentifiedArrayOf<ThreadSummary> = []
        var selectedScope: Scope = .all
        var selectedThreadID: String?
        var chat: ChatFeature.State?
        var searchText = ""
        var isLoading = false
        var errorMessage: String?
        var columnVisibility: NavigationSplitViewVisibility = .all
        var preferredCompactColumn: NavigationSplitViewColumn = .content

        enum Scope: Equatable, Hashable {
            case all
            case project(String)
        }

        var scopeProjectID: String? {
            if case let .project(id) = selectedScope { return id }
            return nil
        }

        var visibleThreads: [ThreadSummary] {
            let base: [ThreadSummary]
            switch selectedScope {
            case .all:
                base = Array(threads)
            case let .project(id):
                base = threads.filter { $0.projectID == id }
            }
            let query = searchText.trimmingCharacters(in: .whitespaces).lowercased()
            guard !query.isEmpty else { return base }
            return base.filter { $0.title.lowercased().contains(query) }
        }
    }

    enum Action: BindableAction {
        case binding(BindingAction<State>)
        case onAppear
        case refresh
        case dataResponse(threads: [ThreadSummary], projects: [Project])
        case loadFailed(FridayError)
        case scopeSelected(State.Scope)
        case threadSelected(String?)
        case newThreadTapped
        case chat(ChatFeature.Action)
        case logoutTapped
        case delegate(Delegate)

        enum Delegate: Equatable {
            case loggedOut
        }
    }

    @Dependency(\.friday) var friday

    var body: some ReducerOf<Self> {
        BindingReducer()
        Reduce { state, action in
            switch action {
            case .binding:
                return .none

            case .onAppear:
                state.isLoading = state.threads.isEmpty
                return .send(.refresh)

            case .refresh:
                return .run { send in
                    async let threads = friday.listThreads()
                    async let projects = friday.listProjects()
                    do {
                        let (loadedThreads, loadedProjects) = try await (threads, projects)
                        await send(.dataResponse(threads: loadedThreads, projects: loadedProjects))
                    } catch let error as FridayError {
                        await send(.loadFailed(error))
                    } catch {
                        await send(.loadFailed(.transport))
                    }
                }

            case let .dataResponse(threads, projects):
                state.isLoading = false
                state.errorMessage = nil
                state.threads = IdentifiedArray(uniqueElements: threads)
                state.projects = IdentifiedArray(uniqueElements: projects)
                if let threadID = state.chat?.threadID, let summary = state.threads[id: threadID] {
                    state.chat?.title = summary.title
                }
                return .none

            case let .loadFailed(error):
                state.isLoading = false
                if error == .unauthorized {
                    return .send(.logoutTapped)
                }
                state.errorMessage = "Couldn't load your conversations."
                return .none

            case let .scopeSelected(scope):
                state.selectedScope = scope
                return .none

            case let .threadSelected(id):
                state.selectedThreadID = id
                guard let id, let summary = state.threads[id: id] else {
                    state.chat = nil
                    return .none
                }
                state.preferredCompactColumn = .detail
                if state.chat?.threadID == id { return .none }
                state.chat = ChatFeature.State(
                    threadID: id,
                    projectID: summary.projectID,
                    title: summary.title
                )
                return .none

            case .newThreadTapped:
                state.selectedThreadID = nil
                state.preferredCompactColumn = .detail
                state.chat = ChatFeature.State(
                    threadID: nil,
                    projectID: state.scopeProjectID,
                    title: "New thread"
                )
                return .none

            case let .chat(.delegate(.threadCreated(id, _))):
                state.selectedThreadID = id
                return .send(.refresh)

            case .chat(.delegate(.threadsNeedRefresh)):
                return .send(.refresh)

            case .chat:
                return .none

            case .logoutTapped:
                return .run { send in
                    await friday.signOut()
                    await send(.delegate(.loggedOut))
                }

            case .delegate:
                return .none
            }
        }
        .ifLet(\.chat, action: \.chat) {
            ChatFeature()
        }
    }
}
