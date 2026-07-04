import SwiftUI

/// The Liquid Glass message input bar. Sends on the trailing button (and on the
/// hardware Return key); turns into a stop control while a run is in flight.
struct Composer: View {
    @Binding var text: String
    @Binding var location: ThreadLocation
    let running: Bool
    let canChangeLocation: Bool
    let canSend: Bool
    let onSend: () -> Void
    let onStop: () -> Void

    @FocusState private var focused: Bool

    var body: some View {
        HStack(alignment: .bottom, spacing: 8) {
            Menu {
                ForEach(ThreadLocation.allCases, id: \.self) { option in
                    Button {
                        location = option
                    } label: {
                        Label(option.label, systemImage: option == .local ? "macwindow" : "icloud")
                    }
                }
            } label: {
                Label(location.label, systemImage: location == .local ? "macwindow" : "icloud")
                    .labelStyle(.iconOnly)
                    .frame(width: 30, height: 30)
            }
            .menuStyle(.button)
            .buttonStyle(.plain)
            .disabled(!canChangeLocation || running)
            .foregroundStyle(canChangeLocation && !running ? .primary : .secondary)
            .padding(.leading, 6)
            .padding(.bottom, 5)

            TextField("Message S.T.R.I.D.E.", text: $text, axis: .vertical)
                .textFieldStyle(.plain)
                .lineLimit(1...6)
                .focused($focused)
                .onSubmit(submit)
                .padding(.vertical, 9)

            if running {
                GlassIconButton(systemName: "stop.fill", prominent: true, tint: .red, action: onStop)
            } else {
                GlassIconButton(systemName: "arrow.up", prominent: true, action: submit)
                    .disabled(!canSend)
                    .opacity(canSend ? 1 : 0.45)
            }
        }
        .padding(5)
        .glassEffect(.regular, in: .rect(cornerRadius: Metrics.composerRadius))
        .overlay(
            RoundedRectangle(cornerRadius: Metrics.composerRadius)
                .strokeBorder(Color.hairline)
        )
    }

    private func submit() {
        guard canSend else { return }
        Haptics.tap()
        onSend()
    }
}

/// Approval request: the agent wants to do something and is waiting for a yes/no.
struct ApprovalCard: View {
    let message: String
    let onApprove: () -> Void
    let onDeny: () -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 14) {
            Label("Approval needed", systemImage: "exclamationmark.shield")
                .font(.subheadline.weight(.semibold))
                .foregroundStyle(.orange)

            Text(message)
                .font(.callout)
                .frame(maxWidth: .infinity, alignment: .leading)

            HStack(spacing: 12) {
                Button(role: .cancel, action: onDeny) {
                    Text("Deny").frame(maxWidth: .infinity)
                }
                .buttonStyle(.glass)

                Button(action: onApprove) {
                    Text("Approve").frame(maxWidth: .infinity)
                }
                .buttonStyle(.glassProminent)
            }
            .controlSize(.large)
        }
        .padding(16)
        .glassEffect(.regular, in: .rect(cornerRadius: 22))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Color.hairline))
    }
}

/// Multiple-choice question the agent asks mid-run.
struct QuizCard: View {
    let question: String
    let options: [String]
    let progress: String
    let onSelect: (String) -> Void

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Label("Question", systemImage: "questionmark.circle")
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(.tint)
                Spacer()
                Text(progress)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Text(question)
                .font(.callout.weight(.medium))
                .frame(maxWidth: .infinity, alignment: .leading)

            VStack(spacing: 8) {
                ForEach(options, id: \.self) { option in
                    Button {
                        Haptics.tap()
                        onSelect(option)
                    } label: {
                        HStack {
                            Text(option)
                                .multilineTextAlignment(.leading)
                            Spacer(minLength: 8)
                            Image(systemName: "chevron.right")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)
                        }
                        .padding(.horizontal, 14)
                        .padding(.vertical, 11)
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .contentShape(.rect)
                    }
                    .buttonStyle(.plain)
                    .background(Color.subtleFill, in: .rect(cornerRadius: 12))
                }
            }
        }
        .padding(16)
        .glassEffect(.regular, in: .rect(cornerRadius: 22))
        .overlay(RoundedRectangle(cornerRadius: 22).strokeBorder(Color.hairline))
    }
}

/// Dismissible inline error.
struct ErrorBanner: View {
    let text: String
    let onDismiss: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            Image(systemName: "exclamationmark.triangle.fill")
                .foregroundStyle(.orange)
            Text(text)
                .font(.footnote)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button(action: onDismiss) {
                Image(systemName: "xmark")
                    .font(.caption.weight(.semibold))
            }
            .buttonStyle(.plain)
            .foregroundStyle(.secondary)
        }
        .padding(12)
        .background(.regularMaterial, in: .rect(cornerRadius: 14))
        .overlay(RoundedRectangle(cornerRadius: 14).strokeBorder(Color.hairline))
    }
}
