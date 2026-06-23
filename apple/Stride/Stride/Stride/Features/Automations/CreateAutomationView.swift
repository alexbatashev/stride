import ComposableArchitecture
import SwiftUI

/// The "new automation" form, presented as a sheet. Fields adapt to the chosen
/// trigger.
struct CreateAutomationView: View {
    @Bindable var store: StoreOf<CreateAutomationFeature>

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    TextField("Name", text: $store.name)
                        .textContentType(.name)
                }

                Section("Trigger") {
                    Picker("Fires on", selection: $store.triggerKind) {
                        ForEach(TriggerKind.allCases, id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }
                    triggerFields
                }

                Section("Action") {
                    Picker("Type", selection: $store.kind) {
                        ForEach(AutomationKind.allCases, id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }
                    Picker("Notify", selection: $store.notifyKind) {
                        ForEach(NotifyKind.allCases, id: \.self) { kind in
                            Text(kind.label).tag(kind)
                        }
                    }
                }

                Section("Task") {
                    payloadEditor
                }

                Section {
                    Toggle("Enabled", isOn: $store.enabled)
                }

                if let error = store.errorMessage {
                    Section {
                        Label(error, systemImage: "exclamationmark.triangle.fill")
                            .font(.footnote)
                            .foregroundStyle(.red)
                    }
                }
            }
            .navigationTitle("New Automation")
            #if os(iOS) || os(visionOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { store.send(.cancelTapped) }
                }
                ToolbarItem(placement: .confirmationAction) {
                    if store.submitting {
                        ProgressView().controlSize(.small)
                    } else {
                        Button("Create") { store.send(.submitTapped) }
                            .disabled(!store.canSubmit)
                    }
                }
            }
        }
    }

    @ViewBuilder
    private var triggerFields: some View {
        switch store.triggerKind {
        case .cron:
            TextField("*/30 * * * *", text: $store.schedule)
                .font(.system(.body, design: .monospaced))
                #if os(iOS) || os(visionOS)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                #endif
            Text("Standard five-field cron expression in UTC.")
                .font(.caption)
                .foregroundStyle(.secondary)
        case .email:
            if store.emailAccounts.isEmpty {
                Text("Add an IMAP account in the web settings to use the email trigger.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            } else {
                Picker("Inbox", selection: $store.emailAccountID) {
                    Text("Choose an inbox").tag("")
                    ForEach(store.emailAccounts) { account in
                        Text("\(account.name) — \(account.email)").tag(account.id)
                    }
                }
                Text("Existing mail is ignored when the automation is created.")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        case .vfsChange:
            TextField("reports/ (empty means all files)", text: $store.watchPath)
                #if os(iOS) || os(visionOS)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()
                #endif
            Text("Leave empty to watch every file.")
                .font(.caption)
                .foregroundStyle(.secondary)
        case .webhook:
            Text("A secret URL is generated after you create the automation.")
                .font(.caption)
                .foregroundStyle(.secondary)
        case .manual:
            Text("Runs only when you trigger it from here.")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var payloadEditor: some View {
        ZStack(alignment: .topLeading) {
            if store.payload.isEmpty {
                Text(store.payloadPrompt)
                    .font(.system(.body, design: store.kind == .python ? .monospaced : .default))
                    .foregroundStyle(.tertiary)
                    .padding(.top, 8)
                    .padding(.leading, 5)
                    .allowsHitTesting(false)
            }
            TextEditor(text: $store.payload)
                .font(.system(.body, design: store.kind == .python ? .monospaced : .default))
                .frame(minHeight: 140)
                #if os(iOS) || os(visionOS)
                .textInputAutocapitalization(store.kind == .python ? .never : .sentences)
                .autocorrectionDisabled(store.kind == .python)
                #endif
        }
    }
}
