import ComposableArchitecture
import SwiftUI

@Reducer
struct HomeFeature {
    @ObservableState
    struct State: Equatable {
        var projects: IdentifiedArrayOf<Project> = []
        var threads: IdentifiedArrayOf<ThreadSummary> = []
        var selection: SidebarSelection = .all
        var selectedThreadID: String?
        var chat: ChatFeature.State?
        var files = FilesFeature.State(scope: .global)
        var automations = AutomationsFeature.State()
        var searchText = ""
        var isLoading = false
        var errorMessage: String?
        var columnVisibility: NavigationSplitViewVisibility = .all
        var preferredCompactColumn: NavigationSplitViewColumn = .content

        enum SidebarSelection: Equatable, Hashable {
            case all
            case project(String)
            case files
            case automations
        }

        var isFiles: Bool { selection == .files }
        var isAutomations: Bool { selection == .automations }

        var scopeProjectID: String? {
            if case let .project(id) = selection { return id }
            return nil
        }

        var visibleThreads: [ThreadSummary] {
            let base: [ThreadSummary]
            switch selection {
            case .all:
                base = Array(threads)
            case let .project(id):
                base = threads.filter { $0.projectID == id }
            case .files, .automations:
                base = []
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
        case loadFailed(StrideError)
        case sidebarSelected(State.SidebarSelection)
        case threadSelected(String?)
        case newThreadTapped
        case chat(ChatFeature.Action)
        case files(FilesFeature.Action)
        case automations(AutomationsFeature.Action)
        case logoutTapped
        case delegate(Delegate)

        enum Delegate: Equatable {
            case loggedOut
        }
    }

    @Dependency(\.stride) var stride

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
                    async let threads = stride.listThreads()
                    async let projects = stride.listProjects()
                    do {
                        let (loadedThreads, loadedProjects) = try await (threads, projects)
                        await send(.dataResponse(threads: loadedThreads, projects: loadedProjects))
                    } catch let error as StrideError {
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

            case let .sidebarSelected(selection):
                state.selection = selection
                if selection == .files || selection == .automations {
                    state.preferredCompactColumn = .content
                }
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

            case .files:
                return .none

            case .automations(.delegate(.automationSelected)):
                state.preferredCompactColumn = .detail
                return .none

            case .automations:
                return .none

            case .logoutTapped:
                return .run { send in
                    await stride.signOut()
                    await send(.delegate(.loggedOut))
                }

            case .delegate:
                return .none
            }
        }
        Scope(state: \.files, action: \.files) {
            FilesFeature()
        }
        Scope(state: \.automations, action: \.automations) {
            AutomationsFeature()
        }
        .ifLet(\.chat, action: \.chat) {
            ChatFeature()
        }
    }
}
