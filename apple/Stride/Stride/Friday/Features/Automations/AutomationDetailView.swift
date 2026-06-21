import ComposableArchitecture
import SwiftUI

/// The detail column: a summary of the selected automation plus its run history
/// and captured output.
struct AutomationDetailView: View {
    @Bindable var store: StoreOf<AutomationsFeature>

    var body: some View {
        Group {
            if let automation = store.selectedAutomation {
                content(for: automation)
            } else {
                placeholder
            }
        }
        #if os(iOS) || os(visionOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
    }

    @ViewBuilder
    private func content(for automation: Automation) -> some View {
        ScrollView {
            VStack(alignment: .leading, spacing: Metrics.gutter) {
                SummaryCard(
                    automation: automation,
                    trigger: automation.triggerDescription(accounts: Array(store.emailAccounts))
                )
                runsSection
            }
            .frame(maxWidth: Metrics.maxReadingWidth, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(Metrics.gutter)
        }
        .navigationTitle(automation.name)
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                Button {
                    store.send(.runTapped(automation.id))
                } label: {
                    if store.hasRunningSelected {
                        ProgressView().controlSize(.small)
                    } else {
                        Label("Run Now", systemImage: "play.fill")
                    }
                }
                .disabled(store.hasRunningSelected)
            }
            ToolbarItem(placement: .automatic) {
                Menu {
                    Button {
                        store.send(.toggleEnabled(automation))
                    } label: {
                        Label(automation.enabled ? "Disable" : "Enable",
                              systemImage: automation.enabled ? "pause.circle" : "play.circle")
                    }
                    Button {
                        store.send(.refresh)
                    } label: {
                        Label("Refresh", systemImage: "arrow.clockwise")
                    }
                    Divider()
                    Button(role: .destructive) {
                        store.send(.deleteTapped(automation))
                    } label: {
                        Label("Delete", systemImage: "trash")
                    }
                } label: {
                    Label("More", systemImage: "ellipsis.circle")
                }
            }
        }
    }

    @ViewBuilder
    private var runsSection: some View {
        HStack {
            Text("Run History")
                .font(.headline)
            Spacer()
            if store.isLoadingRuns {
                ProgressView().controlSize(.small)
            }
        }

        if store.runs.isEmpty {
            ContentUnavailableView {
                Label("No Runs Yet", systemImage: "clock.arrow.circlepath")
            } description: {
                Text("Run the automation to capture its output here.")
            }
            .frame(maxWidth: .infinity, minHeight: 220)
        } else {
            ForEach(Array(store.runs.enumerated()), id: \.element.id) { index, run in
                RunCard(run: run, initiallyExpanded: index == 0)
            }
        }
    }

    private var placeholder: some View {
        ContentUnavailableView {
            Label("Automations", systemImage: "bolt.horizontal.circle")
        } description: {
            Text("Select an automation to see its run history and output.")
        }
    }
}

private struct SummaryCard: View {
    let automation: Automation
    let trigger: String

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 10) {
                Image(systemName: automation.triggerValue.icon)
                    .font(.title3)
                    .foregroundStyle(Color.accentColor)
                VStack(alignment: .leading, spacing: 2) {
                    Text(trigger)
                        .font(.body.weight(.semibold))
                    Text(automation.triggerValue.label)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                Spacer()
                Text(automation.enabled ? "Enabled" : "Paused")
                    .font(.caption.weight(.semibold))
                    .padding(.horizontal, 9)
                    .padding(.vertical, 4)
                    .background(
                        Capsule().fill((automation.enabled ? Color.green : Color.secondary).opacity(0.18))
                    )
                    .foregroundStyle(automation.enabled ? Color.green : Color.secondary)
            }

            Divider()

            HStack(spacing: 18) {
                Detail(label: "Type", value: automation.kindValue == .agent ? "Agent" : "Python")
                Detail(label: "Notify", value: automation.notifyValue.label)
                Detail(label: "Last run", value: automation.lastRunDate?.formatted(date: .abbreviated, time: .shortened) ?? "Never")
            }
        }
        .padding(Metrics.gutter)
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(
            RoundedRectangle(cornerRadius: Metrics.cardRadius)
                .fill(Color.subtleFill)
        )
    }

    private struct Detail: View {
        let label: String
        let value: String

        var body: some View {
            VStack(alignment: .leading, spacing: 2) {
                Text(label.uppercased())
                    .font(.caption2.weight(.semibold))
                    .foregroundStyle(.tertiary)
                Text(value)
                    .font(.subheadline)
                    .lineLimit(1)
            }
        }
    }
}

private struct RunCard: View {
    let run: AutomationRun
    @State private var expanded: Bool

    init(run: AutomationRun, initiallyExpanded: Bool) {
        self.run = run
        _expanded = State(initialValue: initiallyExpanded)
    }

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            ScrollView(.vertical) {
                Text(outputText)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(10)
            }
            .frame(maxHeight: 260)
            .background(
                RoundedRectangle(cornerRadius: 10).fill(Color.subtleFill)
            )
            .padding(.top, 6)
        } label: {
            HStack(spacing: 10) {
                StatusBadge(status: run.statusValue)
                VStack(alignment: .leading, spacing: 2) {
                    Text(run.startedLabel)
                        .font(.subheadline.weight(.medium))
                    Text("Finished \(run.finishedLabel)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
        }
        .padding(.vertical, 6)
    }

    private var outputText: String {
        let trimmed = run.output.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Run completed, but produced no text output." : trimmed
    }
}

private struct StatusBadge: View {
    let status: RunStatus

    var body: some View {
        Text(status.label)
            .font(.caption2.weight(.bold))
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(Capsule().fill(color.opacity(0.18)))
            .foregroundStyle(color)
    }

    private var color: Color {
        switch status {
        case .running: return .orange
        case .success: return .green
        case .failed: return .red
        }
    }
}
