import CoreFriday
import SwiftUI

struct ChatDetailView: View {
    @Bindable var modelData: ModelData
    let conversationID: UUID?

    @State private var draftText: String = ""
    @State private var isSending = false

    private var selectedConversation: Conversation? {
        if let conversationID {
            return modelData.conversations.first(where: { $0.id == conversationID })
        }
        return modelData.sortedConversations.first
    }

    var body: some View {
        Group {
            if let conversation = selectedConversation {
                VStack(spacing: 0) {
                    header(conversation: conversation)
                    Divider()
                    transcript(conversation: conversation)
                    Divider()
                    composer(conversation: conversation)
                }
                .background(
                    LinearGradient(
                        colors: [Color.accentColor.opacity(0.08), Color.clear],
                        startPoint: .top,
                        endPoint: .bottom
                    )
                )
            } else {
                ContentUnavailableView(
                    "Select a Chat",
                    systemImage: "bubble.left.and.text.bubble.right",
                    description: Text("Choose or create a conversation from the middle column.")
                )
            }
        }
        .task {
            if modelData.chatSettings.availableModels.isEmpty {
                await modelData.refreshModels()
            }
        }
    }

    private func header(conversation: Conversation) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title)
                    .font(.title3.weight(.semibold))
                    .accessibilityIdentifier("chatHeaderTitle")

                HStack(spacing: 12) {
                    Picker("Provider", selection: providerSelection) {
                        ForEach(modelData.chatSettings.providers) { provider in
                            Text(provider.name).tag(Optional(provider.id))
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)

                    Picker("Model", selection: modelSelection) {
                        if modelData.chatSettings.availableModels.isEmpty {
                            if modelData.chatSettings.activeModel.isEmpty {
                                Text("No model selected").tag("")
                            } else {
                                Text(modelData.chatSettings.activeModel).tag(modelData.chatSettings.activeModel)
                            }
                        } else {
                            ForEach(modelData.chatSettings.availableModels, id: \.self) { model in
                                Text(model).tag(model)
                            }
                        }
                    }
                    .labelsHidden()
                    .pickerStyle(.menu)

                    Button {
                        Task { await modelData.refreshModels() }
                    } label: {
                        if modelData.chatSettings.isRefreshingModels {
                            ProgressView()
                                .controlSize(.small)
                        } else {
                            Image(systemName: "arrow.clockwise")
                        }
                    }
                    .buttonStyle(.plain)
                    .disabled(modelData.chatSettings.isRefreshingModels)
                    .help("Refresh models")
                }
                .font(.caption)
            }

            Spacer()

            Image(systemName: "apple.intelligence")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
    }

    private var providerSelection: Binding<UUID?> {
        Binding {
            modelData.chatSettings.selectedProviderID
        } set: { newProviderID in
            modelData.chatSettings.selectProvider(newProviderID)
            Task { await modelData.refreshModels() }
        }
    }

    private var modelSelection: Binding<String> {
        Binding {
            modelData.chatSettings.activeModel
        } set: { newModel in
            modelData.chatSettings.setSelectedModel(newModel)
        }
    }

    private func transcript(conversation: Conversation) -> some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 12) {
                    ForEach(conversation.orderedTurns) { turn in
                        TurnBubble(turn: turn)
                            .id(turn.id)
                    }
                }
                .padding(.horizontal, 16)
                .padding(.vertical, 18)
            }
            .onChange(of: conversation.orderedTurns.count) { _, _ in
                guard let lastID = conversation.orderedTurns.last?.id else { return }
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo(lastID, anchor: .bottom)
                }
            }
        }
    }

    private func composer(conversation: Conversation) -> some View {
        VStack(spacing: 10) {
            if let errorMessage = modelData.chatSettings.refreshErrorMessage {
                Text(errorMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
            }

            HStack(alignment: .bottom, spacing: 10) {
                TextField("Message Friday...", text: $draftText, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...6)
                    .accessibilityIdentifier("chatInputField")
                    .disabled(isSending)

                Button(action: { sendMessage(in: conversation) }) {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.system(size: 30))
                }
                .buttonStyle(.plain)
                .disabled(isSending || draftText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
                .accessibilityIdentifier("sendMessageButton")
            }
        }
        .padding(16)
        .background(.thinMaterial)
    }

    private func sendMessage(in conversation: Conversation) {
        let trimmed = draftText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let userSequence = conversation.nextSequenceNumber

        let userTurn = ConversationTurn(
            role: .user,
            text: trimmed,
            sequenceNumber: userSequence
        )
        conversation.turns.append(userTurn)
        conversation.refreshPreview(using: trimmed)

        let turnsForRequest = conversation.orderedTurns

        let assistantTurn = ConversationTurn(
            role: .assistant,
            text: "",
            sequenceNumber: userSequence + 1,
            modelIdentifier: modelData.chatSettings.activeModel
        )
        conversation.turns.append(assistantTurn)

        modelData.persistAll()
        draftText = ""
        isSending = true

        Task {
            defer {
                Task { @MainActor in
                    isSending = false
                }
            }

            do {
                for try await token in modelData.streamAssistantReply(turns: turnsForRequest) {
                    await MainActor.run {
                        assistantTurn.text += token
                        conversation.refreshPreview(using: assistantTurn.text)
                    }
                    await Task.yield()
                }

                try await MainActor.run {
                    if assistantTurn.text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        assistantTurn.text = "(No text returned)"
                    }
                    modelData.persistAll()
                }
            } catch {
                await MainActor.run {
                    assistantTurn.isError = true
                    let base = assistantTurn.text.trimmingCharacters(in: .whitespacesAndNewlines)
                    if base.isEmpty {
                        assistantTurn.text = "Request failed: \(error.localizedDescription)"
                    } else {
                        assistantTurn.text += "\n\nRequest failed: \(error.localizedDescription)"
                    }

                    modelData.persistAll()
                }
            }
        }
    }
}

private struct TurnBubble: View {
    let turn: ConversationTurn

    private var isUser: Bool { turn.role == .user }

    var body: some View {
        HStack(alignment: .bottom) {
            if isUser { Spacer(minLength: 44) }

            VStack(alignment: .leading, spacing: 8) {
                Text(turn.text)
                    .textSelection(.enabled)

                if !turn.attachments.isEmpty {
                    attachments
                }

                if !turn.toolInvocations.isEmpty {
                    tools
                }

                Text(turn.createdAt, style: .time)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(bubbleBackground)
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))

            if !isUser { Spacer(minLength: 44) }
        }
    }

    private var bubbleBackground: some ShapeStyle {
        if turn.isError {
            return AnyShapeStyle(Color.red.opacity(0.15))
        }

        if isUser {
            return AnyShapeStyle(
                LinearGradient(
                    colors: [Color.accentColor.opacity(0.35), Color.accentColor.opacity(0.18)],
                    startPoint: .topLeading,
                    endPoint: .bottomTrailing
                )
            )
        }

        return AnyShapeStyle(.ultraThinMaterial)
    }

    private var attachments: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(turn.attachments) { attachment in
                Label("\(attachment.fileName) · \(attachment.byteCount) B", systemImage: icon(for: attachment.kind))
                    .font(.caption)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .background(.regularMaterial, in: Capsule())
            }
        }
    }

    private var tools: some View {
        VStack(alignment: .leading, spacing: 6) {
            ForEach(turn.toolInvocations) { tool in
                VStack(alignment: .leading, spacing: 4) {
                    Label(tool.name, systemImage: "hammer.fill")
                        .font(.caption.weight(.semibold))
                    Text(tool.argumentsJSON)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                .padding(10)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 12, style: .continuous))
            }
        }
    }

    private func icon(for kind: AttachmentKind) -> String {
        switch kind {
        case .image: return "photo"
        case .file: return "doc"
        case .audio: return "waveform"
        case .video: return "film"
        }
    }
}
