import SwiftUI

/// Shown once after a webhook automation is created. The secret cannot be
/// retrieved later, so it can be copied here.
struct WebhookRevealView: View {
    let reveal: AutomationsFeature.State.WebhookReveal
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            Form {
                Section {
                    Label("Copy the secret now — it is shown only once.", systemImage: "exclamationmark.shield")
                        .font(.subheadline)
                        .foregroundStyle(.orange)
                }

                Section("URL") {
                    CopyableField(label: "Webhook URL", value: reveal.url)
                }

                Section("Secret") {
                    CopyableField(label: "Secret", value: reveal.secret)
                }

                Section {
                    Text("Send a POST request with the `X-Stride-Webhook-Secret` header or a `?token=` query parameter. A JSON body is forwarded to the task.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .navigationTitle("Webhook Created")
            #if os(iOS) || os(visionOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

private struct CopyableField: View {
    let label: String
    let value: String

    var body: some View {
        HStack(spacing: 10) {
            Text(value)
                .font(.system(.footnote, design: .monospaced))
                .textSelection(.enabled)
                .lineLimit(2)
                .frame(maxWidth: .infinity, alignment: .leading)
            Button {
                Clipboard.copy(value)
                Haptics.tap()
            } label: {
                Image(systemName: "doc.on.doc")
            }
            .buttonStyle(.borderless)
            .help("Copy \(label)")
        }
    }
}
