#if os(macOS)
    import CoreFriday
    import SwiftUI

    private struct ModelFormEntry: Identifiable {
        let id = UUID()
        var modelID: String = ""
        var name: String = ""
        var toolsSupport: Bool = false
        var imageSupport: Bool = false
        var thinkingSupport: Bool = false

        init() {}

        init(from config: ModelConfig) {
            modelID = config.id
            name = config.name
            toolsSupport = config.toolsSupport
            imageSupport = config.imageSupport
            thinkingSupport = config.thinkingSupport
        }

        var asModelConfig: ModelConfig {
            ModelConfig(
                id: modelID,
                name: name,
                toolsSupport: toolsSupport,
                imageSupport: imageSupport,
                thinkingSupport: thinkingSupport
            )
        }
    }

    struct ProviderFormSheet: View {
        @Environment(ModelData.self) private var modelData
        @Environment(\.dismiss) private var dismiss

        let existingProvider: ChatProviderConfiguration?

        @State private var name = ""
        @State private var kind: ChatProviderKind = .openAiCompatible
        @State private var baseURL = "https://"
        @State private var token = ""
        @State private var modelEntries: [ModelFormEntry] = [ModelFormEntry()]
        @State private var isFetchingModels = false
        @State private var fetchError: String?

        private var isEditing: Bool { existingProvider != nil }
        private var canFetch: Bool { !baseURL.isEmpty && baseURL != "https://" }

        var body: some View {
            VStack(alignment: .leading, spacing: 0) {
                Text(isEditing ? "Edit Provider" : "Add Provider")
                    .font(.headline)
                    .padding()

                Divider()

                Form {
                    Section("Identity") {
                        TextField("Name", text: $name, prompt: Text("My Provider"))
                        Picker("Kind", selection: $kind) {
                            ForEach(ChatProviderKind.allCases.filter { $0 != .mock }) { k in
                                Text(k.displayName).tag(k)
                            }
                        }
                    }

                    Section("Connection") {
                        if kind != .mock {
                            TextField(
                                "Base URL", text: $baseURL,
                                prompt: Text("https://api.example.com")
                            )
                            .autocorrectionDisabled()
                        }
                        if kind != .ollama && kind != .mock {
                            SecureField("API Key", text: $token, prompt: Text("sk-…"))
                        }
                    }

                    Section {
                        ForEach($modelEntries) { $entry in
                            modelRow(entry: $entry)
                            Divider()
                                .padding(.vertical, 2)
                        }

                        HStack {
                            Button {
                                modelEntries.append(ModelFormEntry())
                            } label: {
                                Label("Add Model", systemImage: "plus")
                                    .font(.callout)
                            }
                            .buttonStyle(.borderless)

                            Spacer()

                            Button(isFetchingModels ? "Fetching…" : "Fetch from API") {
                                Task { await fetchModels() }
                            }
                            .disabled(isFetchingModels || !canFetch)
                        }
                        .padding(.top, 2)

                        if let error = fetchError {
                            Label(error, systemImage: "exclamationmark.triangle.fill")
                                .font(.caption)
                                .foregroundStyle(.red)
                        }
                    } header: {
                        Text("Models")
                    }
                }
                .formStyle(.grouped)

                Divider()

                HStack {
                    Spacer()
                    Button("Cancel") { dismiss() }
                        .keyboardShortcut(.cancelAction)
                    Button(isEditing ? "Save" : "Add") { save() }
                        .keyboardShortcut(.defaultAction)
                        .disabled(name.trimmingCharacters(in: .whitespaces).isEmpty)
                }
                .padding()
            }
            .frame(minWidth: 500, idealWidth: 560, minHeight: 440)
            .onAppear { loadExisting() }
        }

        @ViewBuilder
        private func modelRow(entry: Binding<ModelFormEntry>) -> some View {
            VStack(alignment: .leading, spacing: 6) {
                HStack(spacing: 6) {
                    TextField("Model ID", text: entry.modelID, prompt: Text("gpt-4o"))
                        .autocorrectionDisabled()
                    TextField("Display Name", text: entry.name, prompt: Text("Optional"))
                        .autocorrectionDisabled()
                    Button {
                        let rowID = entry.wrappedValue.id
                        modelEntries.removeAll { $0.id == rowID }
                        if modelEntries.isEmpty { modelEntries = [ModelFormEntry()] }
                    } label: {
                        Image(systemName: "minus.circle.fill")
                            .foregroundStyle(.red)
                    }
                    .buttonStyle(.borderless)
                }
                HStack(spacing: 20) {
                    Toggle("Tools", isOn: entry.toolsSupport)
                    Toggle("Images", isOn: entry.imageSupport)
                    Toggle("Thinking", isOn: entry.thinkingSupport)
                }
                .toggleStyle(.checkbox)
                .font(.callout)
                .foregroundStyle(.secondary)
            }
            .padding(.vertical, 2)
        }

        private func loadExisting() {
            guard let provider = existingProvider else { return }
            name = provider.name
            kind = provider.kind
            baseURL = provider.baseUrl
            token = provider.token
            let stored = modelData.chatSettings.models(for: provider.id)
            modelEntries =
                stored.isEmpty
                ? [ModelFormEntry(from: ModelConfig(id: provider.defaultModel))]
                : stored.map { ModelFormEntry(from: $0) }
            if modelEntries.isEmpty { modelEntries = [ModelFormEntry()] }
        }

        private func fetchModels() async {
            isFetchingModels = true
            fetchError = nil
            let config = ChatProviderConfiguration(
                id: existingProvider?.id ?? UUID().uuidString,
                name: name.isEmpty ? "temp" : name,
                kind: kind,
                baseUrl: baseURL.trimmingCharacters(in: .whitespacesAndNewlines),
                token: token.trimmingCharacters(in: .whitespacesAndNewlines),
                defaultModel: ""
            )
            do {
                let fetched = try await modelData.fetchModels(for: config)
                modelEntries = fetched.map {
                    var e = ModelFormEntry()
                    e.modelID = $0.model
                    e.name = $0.modelName
                    return e
                }
            } catch {
                fetchError = error.localizedDescription
            }
            isFetchingModels = false
        }

        private func save() {
            let configs = modelEntries
                .map { $0.asModelConfig }
                .filter { !$0.id.isEmpty }
            let sanitizedBaseURL: String
            switch kind {
            case .mock: sanitizedBaseURL = ""
            default: sanitizedBaseURL = baseURL.trimmingCharacters(in: .whitespacesAndNewlines)
            }
            let trimmedName = name.trimmingCharacters(in: .whitespaces)
            let providerID = existingProvider?.id ?? UUID().uuidString
            let provider = ChatProviderConfiguration(
                id: providerID,
                name: trimmedName.isEmpty ? "Provider" : trimmedName,
                kind: kind,
                baseUrl: sanitizedBaseURL,
                token: token.trimmingCharacters(in: .whitespacesAndNewlines),
                defaultModel: configs.first?.id ?? ""
            )
            modelData.chatSettings.upsertProvider(provider)
            modelData.chatSettings.setModels(configs, for: providerID)
            if !isEditing {
                modelData.chatSettings.selectProvider(providerID)
            }
            dismiss()
        }
    }
#endif
