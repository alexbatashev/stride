import ComposableArchitecture
import SwiftUI

/// The automation list shown in the content column. Selecting a row reveals its
/// run history in the detail column.
struct AutomationsView: View {
    @Bindable var store: StoreOf<AutomationsFeature>

    var body: some View {
        Group {
            if store.isLoading, store.automations.isEmpty {
                ProgressView()
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
            } else if store.automations.isEmpty {
                emptyState
            } else {
                list
            }
        }
        .navigationTitle("Automations")
        #if os(iOS) || os(visionOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    store.send(.newTapped)
                } label: {
                    Label("New Automation", systemImage: "plus")
                }
            }
        }
        .task { store.send(.onAppear) }
        .sheet(item: $store.scope(state: \.create, action: \.create)) { createStore in
            CreateAutomationView(store: createStore)
        }
        .sheet(item: $store.webhookReveal) { reveal in
            WebhookRevealView(reveal: reveal)
        }
        .alert(
            "Delete \(store.deleteTarget?.name ?? "Automation")?",
            isPresented: deleteBinding,
            presenting: store.deleteTarget
        ) { _ in
            Button("Delete", role: .destructive) { store.send(.confirmDelete) }
            Button("Cancel", role: .cancel) {}
        } message: { _ in
            Text("The automation and all of its run history will be removed.")
        }
    }

    private var list: some View {
        List(selection: selectionBinding) {
            if let error = store.errorMessage {
                ErrorBanner(text: error) { store.send(.dismissError) }
                    .listRowInsets(EdgeInsets(top: 8, leading: 8, bottom: 4, trailing: 8))
                    .listRowSeparator(.hidden)
            }
            ForEach(store.automations) { automation in
                AutomationRow(
                    automation: automation,
                    subtitle: automation.triggerDescription(accounts: Array(store.emailAccounts)),
                    isRunning: store.runningIDs.contains(automation.id)
                )
                .tag(automation.id)
                .contextMenu { rowMenu(automation) }
                .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                    Button(role: .destructive) {
                        store.send(.deleteTapped(automation))
                    } label: {
                        Label("Delete", systemImage: "trash")
                    }
                    Button {
                        store.send(.runTapped(automation.id))
                    } label: {
                        Label("Run", systemImage: "play.fill")
                    }
                    .tint(.green)
                }
            }
        }
        .listStyle(.plain)
        .refreshable { await store.send(.refresh).finish() }
    }

    @ViewBuilder
    private func rowMenu(_ automation: Automation) -> some View {
        Button {
            store.send(.runTapped(automation.id))
        } label: {
            Label("Run Now", systemImage: "play.fill")
        }
        Button {
            store.send(.toggleEnabled(automation))
        } label: {
            Label(automation.enabled ? "Disable" : "Enable",
                  systemImage: automation.enabled ? "pause.circle" : "play.circle")
        }
        Divider()
        Button(role: .destructive) {
            store.send(.deleteTapped(automation))
        } label: {
            Label("Delete", systemImage: "trash")
        }
    }

    private var emptyState: some View {
        ContentUnavailableView {
            Label("No Automations", systemImage: "bolt.horizontal.circle")
        } description: {
            Text("Run tasks on a schedule, webhook, file change, or on demand.")
        } actions: {
            Button("New Automation") { store.send(.newTapped) }
                .buttonStyle(.glassProminent)
        }
    }

    private var selectionBinding: Binding<Automation.ID?> {
        Binding(
            get: { store.selectedID },
            set: { store.send(.automationSelected($0)) }
        )
    }

    private var deleteBinding: Binding<Bool> {
        Binding(
            get: { store.deleteTarget != nil },
            set: { if !$0 { store.deleteTarget = nil } }
        )
    }
}

private struct AutomationRow: View {
    let automation: Automation
    let subtitle: String
    let isRunning: Bool

    var body: some View {
        HStack(spacing: 12) {
            Image(systemName: automation.triggerValue.icon)
                .font(.system(size: 15, weight: .semibold))
                .foregroundStyle(automation.enabled ? Color.accentColor : .secondary)
                .frame(width: 36, height: 36)
                .background(
                    Circle().fill(Color.accentColor.opacity(automation.enabled ? 0.15 : 0.06))
                )

            VStack(alignment: .leading, spacing: 3) {
                HStack(spacing: 6) {
                    Text(automation.name)
                        .font(.body.weight(.semibold))
                        .lineLimit(1)
                    StatusDot(enabled: automation.enabled)
                }
                Text("\(automation.kindValue == .agent ? "Agent" : "Python") · \(subtitle)")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                Text(automation.lastRunLabel)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }

            Spacer()

            if isRunning {
                ProgressView()
                    .controlSize(.small)
            }
        }
        .padding(.vertical, 4)
    }
}

private struct StatusDot: View {
    let enabled: Bool

    var body: some View {
        Text(enabled ? "Enabled" : "Paused")
            .font(.caption2.weight(.semibold))
            .padding(.horizontal, 7)
            .padding(.vertical, 2)
            .background(
                Capsule().fill((enabled ? Color.green : Color.secondary).opacity(0.18))
            )
            .foregroundStyle(enabled ? Color.green : Color.secondary)
    }
}
