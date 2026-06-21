import SwiftUI

/// Renders Markdown produced by the agent into native SwiftUI. Parsing happens
/// in ``MarkdownParser`` (swift-markdown) and is memoized by ``MarkdownCache``;
/// this layer only lays out the resulting blocks.
struct MarkdownView: View {
    let text: String
    var baseURL: URL?

    var body: some View {
        BlockList(blocks: MarkdownCache.blocks(for: text, baseURL: baseURL), baseURL: baseURL)
    }
}

/// A vertical stack of blocks. Used at the top level and recursively for list
/// items and blockquote bodies.
private struct BlockList: View {
    let blocks: [MarkdownBlock]
    var baseURL: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(blocks.enumerated()), id: \.offset) { _, block in
                BlockView(block: block, baseURL: baseURL)
            }
        }
    }
}

private struct BlockView: View {
    let block: MarkdownBlock
    var baseURL: URL?

    var body: some View {
        switch block {
        case let .heading(level, text):
            Text(text)
                .font(headingFont(level))
                .padding(.top, level <= 2 ? 4 : 0)

        case let .paragraph(text):
            Text(text)
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)

        case let .bulletList(items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    marker("•", content: item)
                }
            }

        case let .orderedList(start, items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { index, item in
                    marker("\(start + index).", content: item)
                }
            }

        case let .quote(blocks):
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.accentColor.opacity(0.6))
                    .frame(width: 3)
                BlockList(blocks: blocks, baseURL: baseURL)
                    .foregroundStyle(.secondary)
            }
            .fixedSize(horizontal: false, vertical: true)

        case let .code(language, code):
            CodeBlock(language: language, code: code)

        case let .image(alt, url):
            MarkdownImage(alt: alt, url: resolved(url))

        case .rule:
            Divider().padding(.vertical, 2)
        }
    }

    private func marker(_ label: String, content: [MarkdownBlock]) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 8) {
            Text(label)
                .monospacedDigit()
                .foregroundStyle(.secondary)
            BlockList(blocks: content, baseURL: baseURL)
        }
    }

    private func headingFont(_ level: Int) -> Font {
        switch level {
        case 1: return .title2.bold()
        case 2: return .title3.bold()
        case 3: return .headline
        default: return .subheadline.bold()
        }
    }

    private func resolved(_ url: String) -> URL? {
        if let direct = URL(string: url), direct.scheme != nil { return direct }
        guard let baseURL else { return URL(string: url) }
        return URL(string: url, relativeTo: baseURL)
    }
}

/// A fenced code block with a language label, copy button and horizontal scroll.
private struct CodeBlock: View {
    let language: String?
    let code: String
    @State private var copied = false

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack {
                Text(language?.isEmpty == false ? language! : "code")
                    .font(.caption2.weight(.medium))
                    .foregroundStyle(.secondary)
                Spacer()
                Button {
                    Clipboard.copy(code)
                    withAnimation { copied = true }
                } label: {
                    Label(copied ? "Copied" : "Copy", systemImage: copied ? "checkmark" : "doc.on.doc")
                        .labelStyle(.iconOnly)
                        .font(.caption)
                }
                .buttonStyle(.plain)
                .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 12)
            .padding(.vertical, 7)

            Divider()

            ScrollView(.horizontal, showsIndicators: false) {
                Text(code)
                    .font(.system(.callout, design: .monospaced))
                    .textSelection(.enabled)
                    .padding(12)
            }
        }
        .background(Color.subtleFill, in: .rect(cornerRadius: 12))
        .overlay(
            RoundedRectangle(cornerRadius: 12).strokeBorder(Color.hairline)
        )
    }
}

/// An inline Markdown image loaded from the server, with a graceful placeholder.
private struct MarkdownImage: View {
    let alt: String
    let url: URL?

    var body: some View {
        AsyncImage(url: url) { phase in
            switch phase {
            case .success(let image):
                image
                    .resizable()
                    .scaledToFit()
                    .frame(maxWidth: .infinity)
                    .clipShape(.rect(cornerRadius: Metrics.cardRadius))
            case .failure:
                placeholder(systemName: "photo")
            case .empty:
                placeholder(systemName: "photo")
                    .overlay(ProgressView())
            @unknown default:
                placeholder(systemName: "photo")
            }
        }
        .accessibilityLabel(alt.isEmpty ? "Image" : alt)
    }

    private func placeholder(systemName: String) -> some View {
        RoundedRectangle(cornerRadius: Metrics.cardRadius)
            .fill(Color.subtleFill)
            .frame(height: 160)
            .overlay(Image(systemName: systemName).foregroundStyle(.secondary))
    }
}
