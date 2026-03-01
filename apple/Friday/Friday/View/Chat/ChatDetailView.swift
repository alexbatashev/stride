import CoreFriday
import SwiftUI

struct ChatDetailView: View {
    @Environment(ModelData.self) private var modelData
    let conversation: Conversation

    @State private var draftText: String = ""
    @State private var isSending = false
    
    @State private var showingModelPopover = false

    var body: some View {
        ZStack(alignment: .bottom) {
            // Transcript scrolls behind the composer
            transcript(conversation: conversation)
            
            // Composer floats on top with blur background
            composer(conversation: conversation)
        }
        .task {
            if modelData.chatSettings.availableModels.isEmpty {
                await modelData.refreshModels()
            }
        }
        .toolbar(id: "chat-detail-toolbar") {
            ToolbarItem(id: "model-selector", placement: .automatic) {
                Button {
                    showingModelPopover = true
                } label: {
                    HStack(spacing: 4) {
                        Text(currentModelDisplayName)
                            .lineLimit(1)
                        Image(systemName: "chevron.down")
                            .font(.caption)
                    }
                }
                .popover(isPresented: $showingModelPopover) {
                    modelSelectionPopover
                }
            }
            
            ToolbarItem(id: "spacer", placement: .automatic) {
                Spacer()
            }
            
            ToolbarItem(id: "share", placement: .automatic) {
                Button {} label: {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
            }
        }
        .toolbarRole(.editor)
    }
    
    private var currentModelDisplayName: String {
        if let provider = modelData.chatSettings.activeProvider,
           !modelData.chatSettings.activeModel.isEmpty {
            return "\(provider.name) / \(modelData.chatSettings.activeModel)"
        }
        return "Select Model"
    }
    
    private var availableModels: [LangModel] {
        guard let provider = modelData.chatSettings.activeProvider else {
            return []
        }
        
        return modelData.chatSettings.availableModels.map { modelID in
            LangModel(
                provider: provider.id.uuidString,
                model: modelID,
                providerName: provider.name,
                modelName: modelID
            )
        }
    }
    
    private var modelSelectionPopover: some View {
        VStack(alignment: .leading, spacing: 0) {
            if modelData.chatSettings.isRefreshingModels {
                HStack {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading models...")
                        .foregroundStyle(.secondary)
                }
                .padding()
            } else if availableModels.isEmpty {
                VStack(spacing: 12) {
                    Text("No models available")
                        .foregroundStyle(.secondary)
                    
                    Button("Refresh Models") {
                        Task { await modelData.refreshModels() }
                    }
                }
                .padding()
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(availableModels.prefix(3).enumerated()), id: \.element.model) { index, langModel in
                        Button {
                            selectModel(langModel)
                            showingModelPopover = false
                        } label: {
                            HStack {
                                VStack(alignment: .leading, spacing: 2) {
                                    Text(langModel.readableName())
                                        .font(.body)
                                    Text(langModel.model)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                                Spacer()
                                if langModel.model == modelData.chatSettings.activeModel {
                                    Image(systemName: "checkmark")
                                        .foregroundStyle(.blue)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                        .background(
                            langModel.model == modelData.chatSettings.activeModel 
                                ? Color.accentColor.opacity(0.1) 
                                : Color.clear
                        )
                        
                        if index < min(2, availableModels.count - 1) {
                            Divider()
                                .padding(.leading, 16)
                        }
                    }
                    
                    if availableModels.count > 3 {
                        Divider()
                            .padding(.leading, 16)
                        
                        Button {
                            // TODO: Show full model list
                            showingModelPopover = false
                        } label: {
                            HStack {
                                Text("More...")
                                    .font(.body)
                                Spacer()
                                Image(systemName: "ellipsis")
                                    .foregroundStyle(.secondary)
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.plain)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 10)
                    }
                }
                .padding(.vertical, 8)
            }
            
            Divider()
            
            HStack {
                Button {
                    Task { await modelData.refreshModels() }
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: "arrow.clockwise")
                        Text("Refresh")
                    }
                    .font(.caption)
                }
                .buttonStyle(.borderless)
                .disabled(modelData.chatSettings.isRefreshingModels)
                
                Spacer()
            }
            .padding(.horizontal, 16)
            .padding(.vertical, 10)
        }
        .frame(width: 320)
    }
    
    private func selectModel(_ langModel: LangModel) {
        modelData.chatSettings.setSelectedModel(langModel.model)
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
                .padding(.top, 18)
                .padding(.bottom, 120) // Extra padding so content can scroll behind the composer
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
        VStack(spacing: 0) {
            if let errorMessage = modelData.chatSettings.refreshErrorMessage {
                Text(errorMessage)
                    .font(.caption)
                    .foregroundStyle(.red)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .padding(.horizontal, 12)
                    .padding(.top, 6)
            }

            GlassEffectContainer(spacing: 10) {
                HStack(alignment: .center, spacing: 8) {
                    Button {
                        // TODO: Implement attachment action
                    } label: {
                        Image(systemName: "plus")
                            .font(.system(size: 17, weight: .semibold))
                            .frame(width: 30, height: 30)
                            .glassEffect(.regular.interactive(), in: .circle)
                    }
                    .buttonStyle(.plain)
                    .disabled(isSending)
                    .help("Add attachments")

                    HStack(alignment: .center, spacing: 8) {
                        TextField("Message", text: $draftText, axis: .vertical)
                            .textFieldStyle(.plain)
                            .lineLimit(1...10)
                            .font(.system(size: 14))
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .accessibilityIdentifier("chatInputField")
                            .disabled(isSending)

                        Button {
                            // TODO: Implement voice dictation
                        } label: {
                            Image(systemName: "waveform")
                                .font(.system(size: 15, weight: .medium))
                                .frame(width: 28, height: 28)
                        }
                        .buttonStyle(.plain)
                        .disabled(isSending)
                        .help("Voice input")
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .glassEffect(.regular.interactive(), in: .rect(cornerRadius: 20, style: .continuous))

                    Group {
                        if draftText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            Button {
                                // TODO: Show emoji picker
                            } label: {
                                Image(systemName: "face.smiling")
                                    .font(.system(size: 17, weight: .medium))
                                    .frame(width: 30, height: 30)
                                    .glassEffect(.regular.interactive(), in: .circle)
                            }
                            .buttonStyle(.plain)
                            .help("Emoji")
                            .transition(.opacity.combined(with: .scale))
                        } else {
                            Button(action: { sendMessage(in: conversation) }) {
                                Image(systemName: "arrow.up")
                                    .font(.system(size: 16, weight: .bold))
                                    .frame(width: 30, height: 30)
                                    .glassEffect(.regular.tint(.accentColor).interactive(), in: .circle)
                            }
                            .buttonStyle(.plain)
                            .disabled(isSending)
                            .accessibilityIdentifier("sendMessageButton")
                            .help("Send message")
                            .transition(.opacity.combined(with: .scale))
                        }
                    }
                    .animation(.spring(duration: 0.2), value: draftText.isEmpty)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
        }
        .padding(.bottom, 8)
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

                await MainActor.run {
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
