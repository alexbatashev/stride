import CoreFriday
import SwiftUI

struct ChatSettingsView: View {
    @Environment(ModelData.self) private var modelData
    @Environment(\.dismiss) private var dismiss

    @State private var providerID: UUID?
    @State private var name: String = ""
    @State private var kind: ChatProviderKind = .openAICompatible
    @State private var baseURL: String = ""
    @State private var token: String = ""
    @State private var defaultModel: String = ""

    var body: some View {
        VStack(spacing: 0) {
            Form {
                Section("Provider") {
                    Picker("Saved Providers", selection: $providerID) {
                        ForEach(modelData.chatSettings.providers) { provider in
                            Text(provider.name).tag(Optional(provider.id))
                        }
                    }
                    .onChange(of: providerID) { _, newValue in
                        modelData.chatSettings.selectProvider(newValue)
                        loadSelectedProvider()
                    }

                    HStack {
                        Button("Add") {
                            let created = modelData.chatSettings.addProviderTemplate()
                            providerID = created.id
                            loadSelectedProvider()
                        }

                        Button("Delete", role: .destructive) {
                            modelData.chatSettings.removeSelectedProvider()
                            providerID = modelData.chatSettings.selectedProviderID
                            loadSelectedProvider()
                        }

                        Spacer()
                    }
                }

                Section("Configuration") {
                    TextField("Name", text: $name)
                    Picker("Provider Type", selection: $kind) {
                        ForEach(ChatProviderKind.allCases) { providerKind in
                            Text(providerKind.displayName).tag(providerKind)
                        }
                    }

                    if kind != .mock {
                        TextField("Base URL", text: $baseURL)
                    }

                    if kind != .ollama && kind != .mock {
                        SecureField("API Token", text: $token)
                    } else {
                        TextField("API Token (optional)", text: $token)
                    }

                    TextField("Default Model", text: $defaultModel)
                }

                Section("Models") {
                    HStack {
                        Button(modelData.chatSettings.isRefreshingModels ? "Refreshing..." : "Refresh Models") {
                            saveEditsToStore()
                            Task { await modelData.refreshModels() }
                        }
                        .disabled(modelData.chatSettings.isRefreshingModels)

                        Spacer()
                    }

                    if let error = modelData.chatSettings.refreshErrorMessage {
                        Text(error)
                            .font(.caption)
                            .foregroundStyle(.red)
                    }

                    if !modelData.chatSettings.availableModels.isEmpty {
                        Picker("Detected Models", selection: $defaultModel) {
                            ForEach(modelData.chatSettings.availableModels, id: \.self) { model in
                                Text(model).tag(model)
                            }
                        }
                    }
                }
            }

            HStack {
                Spacer()
                Button("Cancel") {
                    dismiss()
                }
                Button("Save") {
                    saveEditsToStore()
                    dismiss()
                }
                .keyboardShortcut(.defaultAction)
            }
            .padding(16)
            .background(.regularMaterial)
        }
        .frame(minWidth: 560, minHeight: 520)
        .onAppear {
            providerID = modelData.chatSettings.selectedProviderID
            loadSelectedProvider()
        }
    }

    private func loadSelectedProvider() {
        guard let provider = modelData.chatSettings.activeProvider else { return }
        providerID = provider.id
        name = provider.name
        kind = provider.kind
        baseURL = provider.baseURL
        token = provider.token
        defaultModel = provider.defaultModel
    }

    private func saveEditsToStore() {
        guard let providerID else { return }

        let sanitizedBaseURL: String
        switch kind {
        case .mock:
            sanitizedBaseURL = ""
        default:
            sanitizedBaseURL = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
        }

        let trimmedModel = defaultModel.trimmingCharacters(in: .whitespacesAndNewlines)

        let updated = ChatProviderConfiguration(
            id: providerID,
            name: name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "Provider" : name.trimmingCharacters(in: .whitespacesAndNewlines),
            kind: kind,
            baseURL: sanitizedBaseURL,
            token: token.trimmingCharacters(in: .whitespacesAndNewlines),
            defaultModel: trimmedModel
        )

        modelData.chatSettings.upsertProvider(updated)
        modelData.chatSettings.selectProvider(updated.id)
        modelData.chatSettings.setSelectedModel(trimmedModel)
        self.defaultModel = modelData.chatSettings.activeModel
    }
}
