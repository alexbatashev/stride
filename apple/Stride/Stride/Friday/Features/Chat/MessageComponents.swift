import SwiftUI

/// Routes a stored message to the right presentation based on its role.
/// `Equatable` so that, wrapped in `.equatable()`, an already-committed row skips
/// body evaluation (and the Markdown parse it triggers) when an unrelated part of
/// the chat — a streaming token, the typing indicator — changes.
struct MessageRow: View, Equatable {
    let message: ChatMessage
    let baseURL: URL?

    var body: some View {
        switch message.role {
        case .user:
            UserBubble(text: message.content)
        case .tool:
            ToolOutputCard(name: message.toolName ?? "Tool output", content: message.content)
        case .agent, .system:
            AgentMessage(
                content: message.content,
                thinking: message.thinking,
                toolName: message.toolName,
                baseURL: baseURL
            )
        }
    }
}

/// The live, still-streaming agent turn.
struct StreamingRow: View {
    let streaming: ChatFeature.State.Streaming
    let baseURL: URL?

    var body: some View {
        if streaming.content.isEmpty, streaming.thinking.isEmpty {
            TypingIndicator()
        } else {
            AgentMessage(
                content: streaming.content,
                thinking: streaming.thinking.isEmpty ? nil : streaming.thinking,
                toolName: nil,
                baseURL: baseURL,
                isStreaming: true
            )
        }
    }
}

private struct UserBubble: View {
    let text: String

    var body: some View {
        HStack {
            Spacer(minLength: 48)
            Text(text)
                .textSelection(.enabled)
                .foregroundStyle(.white)
                .padding(.horizontal, 15)
                .padding(.vertical, 10)
                .background(Color.accentColor, in: .rect(cornerRadius: Metrics.bubbleRadius))
                .frame(maxWidth: Metrics.maxBubbleWidth, alignment: .trailing)
        }
    }
}

private struct AgentMessage: View {
    let content: String
    var thinking: String?
    var toolName: String?
    let baseURL: URL?
    var isStreaming: Bool = false

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            if let thinking, !thinking.isEmpty {
                ThinkingDisclosure(text: thinking)
            }
            if !content.isEmpty {
                HStack(alignment: .bottom, spacing: 4) {
                    MarkdownView(text: content, baseURL: baseURL)
                    if isStreaming {
                        BlinkingCursor()
                    }
                }
            }
            if let toolName {
                ToolCallChip(name: toolName)
            }
        }
        .frame(maxWidth: .infinity, alignment: .leading)
    }
}

private struct ThinkingDisclosure: View {
    let text: String
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            Text(text)
                .font(.callout)
                .foregroundStyle(.secondary)
                .textSelection(.enabled)
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.top, 4)
        } label: {
            Label("Reasoning", systemImage: "brain")
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .background(Color.subtleFill, in: .rect(cornerRadius: Metrics.cardRadius))
    }
}

private struct ToolCallChip: View {
    let name: String

    var body: some View {
        HStack(spacing: 6) {
            Image(systemName: "wrench.and.screwdriver")
                .font(.caption2)
            Text(name)
                .font(.caption.weight(.medium))
        }
        .foregroundStyle(.secondary)
        .padding(.horizontal, 10)
        .padding(.vertical, 5)
        .background(Color.subtleFill, in: .capsule)
    }
}

private struct ToolOutputCard: View {
    let name: String
    let content: String
    @State private var expanded = false

    var body: some View {
        DisclosureGroup(isExpanded: $expanded) {
            ScrollView(.horizontal, showsIndicators: false) {
                Text(content)
                    .font(.system(.caption, design: .monospaced))
                    .textSelection(.enabled)
                    .padding(.top, 6)
            }
        } label: {
            Label(name, systemImage: "terminal")
                .font(.subheadline.weight(.medium))
                .foregroundStyle(.secondary)
        }
        .padding(12)
        .background(Color.subtleFill, in: .rect(cornerRadius: Metrics.cardRadius))
    }
}

/// Animated three-dot indicator for "agent is thinking".
struct TypingIndicator: View {
    @State private var phase = 0

    var body: some View {
        HStack(spacing: 5) {
            ForEach(0..<3, id: \.self) { index in
                Circle()
                    .fill(.secondary)
                    .frame(width: 7, height: 7)
                    .opacity(phase == index ? 1 : 0.3)
            }
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 12)
        .background(Color.subtleFill, in: .capsule)
        .task {
            while !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(280))
                withAnimation(.easeInOut(duration: 0.25)) {
                    phase = (phase + 1) % 3
                }
            }
        }
    }
}

private struct BlinkingCursor: View {
    @State private var visible = true

    var body: some View {
        RoundedRectangle(cornerRadius: 1)
            .fill(.tint)
            .frame(width: 8, height: 16)
            .opacity(visible ? 1 : 0)
            .task {
                while !Task.isCancelled {
                    try? await Task.sleep(for: .milliseconds(520))
                    withAnimation(.easeInOut(duration: 0.2)) { visible.toggle() }
                }
            }
    }
}

/// Shows which tool the agent is currently running.
struct ToolActivityRow: View {
    let name: String

    var body: some View {
        HStack(spacing: 8) {
            ProgressView().controlSize(.small)
            Text("Running \(name)…")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(.horizontal, 14)
        .padding(.vertical, 9)
        .background(Color.subtleFill, in: .capsule)
    }
}
