#if os(macOS)
    import CoreFriday
    import SwiftUI

    struct ProvidersSettingsView: View {
        @Environment(ModelData.self) private var modelData
        @State private var selectedIDs: Set<String> = []
        @State private var isAddingProvider = false
        @State private var editingProvider: ChatProviderConfiguration?

        private var selectedProvider: ChatProviderConfiguration? {
            guard selectedIDs.count == 1, let id = selectedIDs.first else { return nil }
            return modelData.chatSettings.providers.first { $0.id == id }
        }

        var body: some View {
            VStack(spacing: 0) {
                Table(modelData.chatSettings.providers, selection: $selectedIDs) {
                    TableColumn("ID") { provider in
                        Text(provider.id)
                            .font(.body.monospaced())
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .help(provider.id)
                    }
                    .width(min: 60, ideal: 100, max: 140)

                    TableColumn("Name") { provider in
                        Text(provider.name)
                    }
                    .width(min: 80, ideal: 140)

                    TableColumn("Models") { provider in
                        let list = modelData.chatSettings.models(for: provider.id)
                        let display = list.map { $0.name.isEmpty ? $0.id : $0.name }
                            .joined(separator: ", ")
                        Text(display.isEmpty ? "—" : display)
                            .foregroundStyle(display.isEmpty ? .tertiary : .primary)
                            .lineLimit(1)
                    }
                    .width(min: 80, ideal: 200)

                    TableColumn("Kind") { provider in
                        Text(provider.kind.displayName)
                            .foregroundStyle(.secondary)
                    }
                    .width(min: 80, ideal: 120, max: 160)
                }

                Divider()

                HStack(spacing: 0) {
                    Button {
                        isAddingProvider = true
                    } label: {
                        Image(systemName: "plus")
                            .frame(width: 22, height: 22)
                    }
                    .buttonStyle(.borderless)
                    .padding(.horizontal, 4)
                    .help("Add Provider")

                    Divider()
                        .frame(height: 16)

                    Button {
                        removeSelected()
                    } label: {
                        Image(systemName: "minus")
                            .frame(width: 22, height: 22)
                    }
                    .buttonStyle(.borderless)
                    .padding(.horizontal, 4)
                    .disabled(selectedIDs.isEmpty)
                    .help("Remove Selected")

                    Divider()
                        .frame(height: 16)

                    Button {
                        editingProvider = selectedProvider
                    } label: {
                        Image(systemName: "pencil")
                            .frame(width: 22, height: 22)
                    }
                    .buttonStyle(.borderless)
                    .padding(.horizontal, 4)
                    .disabled(selectedProvider == nil)
                    .help("Edit Provider")

                    Spacer()
                }
                .padding(.vertical, 4)
                .background(.bar)
            }
            .navigationTitle("Models")
            .sheet(isPresented: $isAddingProvider) {
                ProviderFormSheet(existingProvider: nil)
                    .environment(modelData)
            }
            .sheet(item: $editingProvider) { provider in
                ProviderFormSheet(existingProvider: provider)
                    .environment(modelData)
            }
        }

        private func removeSelected() {
            for id in selectedIDs {
                modelData.chatSettings.providers.removeAll { $0.id == id }
                modelData.chatSettings.providerModels.removeValue(forKey: id)
                if modelData.chatSettings.selectedProviderID == id {
                    modelData.chatSettings.selectedProviderID =
                        modelData.chatSettings.providers.first?.id
                }
            }
            modelData.chatSettings.onChange?()
            selectedIDs = []
        }
    }
#endif
