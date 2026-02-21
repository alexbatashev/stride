import SwiftUI
import SwiftData

struct ChatView: View {
    let conversationID: UUID?

    @Environment(\.modelContext) private var modelContext
    @Query(sort: [SortDescriptor(\Conversation.updatedAt, order: .reverse)])
    private var conversations: [Conversation]

    @State private var draftText: String = ""
    @State private var includeDemoAttachment = false
    @State private var includeDemoToolCall = false

    private var selectedConversation: Conversation? {
        if let conversationID {
            return conversations.first(where: { $0.id == conversationID })
        }
        return conversations.first
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
    }

    private func header(conversation: Conversation) -> some View {
        HStack {
            VStack(alignment: .leading, spacing: 2) {
                Text(conversation.title)
                    .font(.title3.weight(.semibold))
                    .accessibilityIdentifier("chatHeaderTitle")
                Text("Local prototype · SwiftData")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            Image(systemName: "apple.intelligence")
                .font(.title3)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 20)
        .padding(.vertical, 12)
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
            .onChange(of: conversation.orderedTurns.count) {
                _, _ in
                guard let lastID = conversation.orderedTurns.last?.id else { return }
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo(lastID, anchor: .bottom)
                }
            }
        }
    }

    private func composer(conversation: Conversation) -> some View {
        VStack(spacing: 10) {
            HStack(spacing: 8) {
                Toggle(isOn: $includeDemoAttachment) {
                    Label("Attachment", systemImage: "paperclip")
                }
                .toggleStyle(.button)
                .buttonStyle(.bordered)

                Toggle(isOn: $includeDemoToolCall) {
                    Label("Tool", systemImage: "wrench.and.screwdriver")
                }
                .toggleStyle(.button)
                .buttonStyle(.bordered)

                Spacer()
            }

            HStack(alignment: .bottom, spacing: 10) {
                TextField("Message Friday...", text: $draftText, axis: .vertical)
                    .textFieldStyle(.roundedBorder)
                    .lineLimit(1...6)
                    .accessibilityIdentifier("chatInputField")

                Button(action: { sendMessage(in: conversation) }) {
                    Image(systemName: "arrow.up.circle.fill")
                        .font(.system(size: 30))
                }
                .buttonStyle(.plain)
                .disabled(draftText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
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
        let assistantSequence = userSequence + 1

        let userTurn = ConversationTurn(
            role: .user,
            text: trimmed,
            sequenceNumber: userSequence,
            conversation: conversation
        )
        conversation.turns.append(userTurn)
        modelContext.insert(userTurn)

        if includeDemoAttachment {
            let attachment = TurnAttachment(
                kind: .file,
                fileName: "requirements.txt",
                mimeType: "text/plain",
                localPath: "/local/stub/requirements.txt",
                byteCount: 1_024,
                turn: userTurn
            )
            userTurn.attachments.append(attachment)
            modelContext.insert(attachment)
        }

        let assistantText = includeDemoToolCall
            ? "Stub response prepared. I also simulated a local tool call for this turn."
            : "Stub response: this is where streamed model output will appear once inference is connected."

        let assistantTurn = ConversationTurn(
            role: .assistant,
            text: assistantText,
            sequenceNumber: assistantSequence,
            modelIdentifier: "local.stub.v1",
            conversation: conversation
        )
        conversation.turns.append(assistantTurn)
        modelContext.insert(assistantTurn)

        if includeDemoToolCall {
            let tool = ToolInvocation(
                name: "fetch_context",
                argumentsJSON: "{\"query\":\"\(trimmed.replacingOccurrences(of: "\"", with: "\\\\\""))\"}",
                resultJSON: "{\"status\":\"ok\",\"source\":\"local-stub\"}",
                status: .completed,
                endedAt: .now,
                turn: assistantTurn
            )
            assistantTurn.toolInvocations.append(tool)
            modelContext.insert(tool)
        }

        conversation.refreshPreview(using: trimmed)

        do {
            try modelContext.save()
            draftText = ""
            includeDemoAttachment = false
            includeDemoToolCall = false
        } catch {
            assertionFailure("Failed to save chat turn: \(error)")
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
