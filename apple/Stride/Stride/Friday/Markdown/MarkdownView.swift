import SwiftUI

/// Renders Markdown produced by the agent into native SwiftUI. Inline spans
/// (bold, italic, code, links) come from `AttributedString`'s Markdown parser;
/// block layout (code fences, lists, quotes, images) is laid out by hand.
struct MarkdownView: View {
    let text: String
    var baseURL: URL?

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(MarkdownParser.parse(text).enumerated()), id: \.offset) { _, block in
                blockView(block)
            }
        }
    }

    @ViewBuilder
    private func blockView(_ block: MarkdownBlock) -> some View {
        switch block {
        case .heading(let level, let text):
            Text(inline(text))
                .font(headingFont(level))
                .padding(.top, level <= 2 ? 4 : 0)

        case .paragraph(let text):
            Text(inline(text))
                .textSelection(.enabled)
                .fixedSize(horizontal: false, vertical: true)

        case .bulletList(let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(Array(items.enumerated()), id: \.offset) { _, item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("•").foregroundStyle(.secondary)
                        Text(inline(item)).fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

        case .orderedList(let items):
            VStack(alignment: .leading, spacing: 6) {
                ForEach(items) { item in
                    HStack(alignment: .firstTextBaseline, spacing: 8) {
                        Text("\(item.number).")
                            .monospacedDigit()
                            .foregroundStyle(.secondary)
                        Text(inline(item.text)).fixedSize(horizontal: false, vertical: true)
                    }
                }
            }

        case .quote(let text):
            HStack(alignment: .top, spacing: 10) {
                RoundedRectangle(cornerRadius: 2)
                    .fill(Color.accentColor.opacity(0.6))
                    .frame(width: 3)
                Text(inline(text))
                    .foregroundStyle(.secondary)
                    .fixedSize(horizontal: false, vertical: true)
            }
            .fixedSize(horizontal: false, vertical: true)

        case .code(let language, let code):
            CodeBlock(language: language, code: code)

        case .image(let alt, let url):
            MarkdownImage(alt: alt, url: resolved(url))

        case .rule:
            Divider().padding(.vertical, 2)
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

    private func inline(_ string: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            allowsExtendedAttributes: true,
            interpretedSyntax: .inlineOnlyPreservingWhitespace,
            failurePolicy: .returnPartiallyParsedIfPossible
        )
        guard var attributed = try? AttributedString(markdown: string, options: options) else {
            return AttributedString(string)
        }
        guard let baseURL else { return attributed }

        let ranges = attributed.runs.compactMap { run -> (Range<AttributedString.Index>, URL)? in
            guard let link = run.link, link.scheme == nil,
                  let absolute = URL(string: link.relativeString, relativeTo: baseURL)
            else { return nil }
            return (run.range, absolute)
        }
        for (range, url) in ranges {
            attributed[range].link = url
        }
        return attributed
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
