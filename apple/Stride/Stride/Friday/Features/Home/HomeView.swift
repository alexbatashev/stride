import ComposableArchitecture
import SwiftUI

/// The Mail-inspired three-column shell: mailboxes (projects) → conversation
/// list → conversation. Collapses to a navigation stack on iPhone.
struct HomeView: View {
    @Bindable var store: StoreOf<HomeFeature>

    var body: some View {
        NavigationSplitView(
            columnVisibility: $store.columnVisibility,
            preferredCompactColumn: $store.preferredCompactColumn
        ) {
            SidebarView(store: store)
                .navigationSplitViewColumnWidth(min: 220, ideal: 260, max: 320)
        } content: {
            Group {
                if store.isFiles {
                    FilesView(store: store.scope(state: \.files, action: \.files))
                } else {
                    ThreadListView(store: store)
                }
            }
            .navigationSplitViewColumnWidth(min: 300, ideal: 360, max: 460)
        } detail: {
            NavigationStack {
                detail
            }
        }
        .task { store.send(.onAppear) }
    }

    @ViewBuilder
    private var detail: some View {
        if store.isFiles {
            FilesDetailPlaceholder()
        } else if let chatStore = store.scope(state: \.chat, action: \.chat) {
            ChatView(store: chatStore)
                .id(chatStore.draftID)
        } else {
            ChatEmptyState { store.send(.newThreadTapped) }
        }
    }
}

private struct SidebarView: View {
    @Bindable var store: StoreOf<HomeFeature>

    var body: some View {
        List(selection: scopeBinding) {
            Section {
                Label("All Conversations", systemImage: "tray.full")
                    .badge(store.threads.count)
                    .tag(HomeFeature.State.SidebarSelection.all)
            }

            if !store.projects.isEmpty {
                Section("Projects") {
                    ForEach(store.projects) { project in
                        Label(project.title, systemImage: "folder")
                            .badge(store.threads.filter { $0.projectID == project.id }.count)
                            .tag(HomeFeature.State.SidebarSelection.project(project.id))
                    }
                }
            }

            Section("Library") {
                Label("Files", systemImage: "folder.fill")
                    .tag(HomeFeature.State.SidebarSelection.files)
            }
        }
        .listStyle(.sidebar)
        .navigationTitle("Friday")
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    store.send(.newThreadTapped)
                } label: {
                    Label("New Conversation", systemImage: "square.and.pencil")
                }
            }
            ToolbarItem(placement: .automatic) {
                Menu {
                    Button {
                        store.send(.refresh)
                    } label: {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    Divider()
                    Button(role: .destructive) {
                        store.send(.logoutTapped)
                    } label: {
                        Label("Sign Out", systemImage: "rectangle.portrait.and.arrow.right")
                    }
                } label: {
                    Label("Account", systemImage: "person.crop.circle")
                }
            }
        }
    }

    private var scopeBinding: Binding<HomeFeature.State.SidebarSelection?> {
        Binding(
            get: { store.selection },
            set: { if let selection = $0 { store.send(.sidebarSelected(selection)) } }
        )
    }
}

private struct ThreadListView: View {
    @Bindable var store: StoreOf<HomeFeature>

    var body: some View {
        List(selection: threadBinding) {
            ForEach(store.visibleThreads) { thread in
                ThreadRow(title: thread.title, subtitle: subtitle(for: thread))
                    .tag(thread.id)
            }
        }
        .listStyle(.plain)
        .navigationTitle(scopeTitle)
        #if os(iOS) || os(visionOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .searchable(text: $store.searchText, prompt: "Search conversations")
        .overlay {
            if store.isLoading {
                ProgressView()
            } else if store.visibleThreads.isEmpty {
                emptyState
            }
        }
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    store.send(.newThreadTapped)
                } label: {
                    Label("New Conversation", systemImage: "square.and.pencil")
                }
            }
        }
        .refreshable { await store.send(.refresh).finish() }
    }

    @ViewBuilder
    private var emptyState: some View {
        if store.searchText.isEmpty {
            ContentUnavailableView {
                Label("No Conversations", systemImage: "bubble.left.and.bubble.right")
            } description: {
                Text("Start a new conversation with Friday.")
            } actions: {
                Button("New Conversation") { store.send(.newThreadTapped) }
                    .buttonStyle(.glassProminent)
            }
        } else {
            ContentUnavailableView.search(text: store.searchText)
        }
    }

    private var threadBinding: Binding<String?> {
        Binding(
            get: { store.selectedThreadID },
            set: { store.send(.threadSelected($0)) }
        )
    }

    private var scopeTitle: String {
        switch store.selection {
        case .all:
            return "All Conversations"
        case let .project(id):
            return store.projects[id: id]?.title ?? "Project"
        case .files:
            return "Files"
        }
    }

    private func subtitle(for thread: ThreadSummary) -> String {
        guard let projectID = thread.projectID, let project = store.projects[id: projectID] else {
            return "Friday"
        }
        return project.title
    }
}

private struct ThreadRow: View {
    let title: String
    let subtitle: String

    var body: some View {
        HStack(spacing: 12) {
            Circle()
                .fill(
                    LinearGradient(
                        colors: [Color.accentColor, Color.accentColor.opacity(0.6)],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
                .frame(width: 36, height: 36)
                .overlay(
                    Image(systemName: "sparkles")
                        .font(.system(size: 15, weight: .semibold))
                        .foregroundStyle(.white)
                )

            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.body.weight(.semibold))
                    .lineLimit(1)
                Text(subtitle)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct FilesDetailPlaceholder: View {
    var body: some View {
        ContentUnavailableView {
            Label("Your Files", systemImage: "folder")
        } description: {
            Text("Select a file to preview it, or upload a new one.")
        }
    }
}

private struct ChatEmptyState: View {
    var onNew: () -> Void

    var body: some View {
        ContentUnavailableView {
            Label("Friday", systemImage: "sparkles")
        } description: {
            Text("Select a conversation or start a new one to begin.")
        } actions: {
            Button("New Conversation", systemImage: "square.and.pencil") { onNew() }
                .buttonStyle(.glassProminent)
                .controlSize(.large)
        }
    }
}
