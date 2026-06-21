import Foundation
import Markdown

/// A block-level element ready to render. Inline spans (bold, italic, code,
/// links) are already baked into the `AttributedString` payloads, so the view
/// layer never parses anything itself.
enum MarkdownBlock {
    case heading(level: Int, text: AttributedString)
    case paragraph(AttributedString)
    case bulletList(items: [[MarkdownBlock]])
    case orderedList(start: Int, items: [[MarkdownBlock]])
    case quote([MarkdownBlock])
    case code(language: String?, code: String)
    case image(alt: String, url: String)
    case rule
}

/// Turns Markdown into ``MarkdownBlock``s using swift-markdown's CommonMark
/// parser. Replaces a hand-rolled line scanner, so nested lists, blockquotes and
/// inline emphasis all parse correctly. Results are memoized by ``MarkdownCache``.
enum MarkdownParser {
    static func parse(_ source: String, baseURL: URL?) -> [MarkdownBlock] {
        let document = Document(parsing: source)
        return blocks(Array(document.children), baseURL: baseURL)
    }

    // MARK: - Blocks

    private static func blocks(_ markups: [Markup], baseURL: URL?) -> [MarkdownBlock] {
        markups.compactMap { block($0, baseURL: baseURL) }
    }

    private static func block(_ markup: Markup, baseURL: URL?) -> MarkdownBlock? {
        switch markup {
        case let heading as Heading:
            return .heading(level: heading.level, text: inline(heading, baseURL: baseURL))
        case let paragraph as Paragraph:
            return standaloneImage(paragraph) ?? .paragraph(inline(paragraph, baseURL: baseURL))
        case let list as UnorderedList:
            return .bulletList(items: items(of: list.listItems, baseURL: baseURL))
        case let list as OrderedList:
            return .orderedList(start: Int(list.startIndex), items: items(of: list.listItems, baseURL: baseURL))
        case let quote as BlockQuote:
            return .quote(blocks(Array(quote.children), baseURL: baseURL))
        case let code as CodeBlock:
            let language = code.language.flatMap { $0.isEmpty ? nil : $0 }
            return .code(language: language, code: trimmingTrailingNewlines(code.code))
        case is ThematicBreak:
            return .rule
        case let html as HTMLBlock:
            return .paragraph(AttributedString(html.rawHTML.trimmingCharacters(in: .whitespacesAndNewlines)))
        default:
            return nil
        }
    }

    private static func items(of listItems: some Sequence<ListItem>, baseURL: URL?) -> [[MarkdownBlock]] {
        listItems.map { blocks(Array($0.children), baseURL: baseURL) }
    }

    private static func standaloneImage(_ paragraph: Paragraph) -> MarkdownBlock? {
        let children = Array(paragraph.children)
        guard children.count == 1, let image = children.first as? Markdown.Image,
              let source = image.source else { return nil }
        return .image(alt: image.plainText, url: source)
    }

    private static func trimmingTrailingNewlines(_ string: String) -> String {
        var result = string
        while result.hasSuffix("\n") { result.removeLast() }
        return result
    }

    // MARK: - Inline

    private static func inline(_ markup: Markup, baseURL: URL?) -> AttributedString {
        descend(markup.children, intent: [], link: nil, baseURL: baseURL)
    }

    private static func descend(
        _ children: some Sequence<Markup>,
        intent: InlinePresentationIntent,
        link: URL?,
        baseURL: URL?
    ) -> AttributedString {
        var result = AttributedString()
        for child in children {
            result.append(fragment(child, intent: intent, link: link, baseURL: baseURL))
        }
        return result
    }

    private static func fragment(
        _ markup: Markup,
        intent: InlinePresentationIntent,
        link: URL?,
        baseURL: URL?
    ) -> AttributedString {
        switch markup {
        case let text as Markdown.Text:
            return styled(text.string, intent: intent, link: link)
        case let code as InlineCode:
            return styled(code.code, intent: intent.union(.code), link: link)
        case let emphasis as Emphasis:
            return descend(emphasis.children, intent: intent.union(.emphasized), link: link, baseURL: baseURL)
        case let strong as Strong:
            return descend(strong.children, intent: intent.union(.stronglyEmphasized), link: link, baseURL: baseURL)
        case let strike as Strikethrough:
            return descend(strike.children, intent: intent.union(.strikethrough), link: link, baseURL: baseURL)
        case let anchor as Markdown.Link:
            let resolved = anchor.destination.flatMap { resolveLink($0, baseURL: baseURL) }
            return descend(anchor.children, intent: intent, link: resolved ?? link, baseURL: baseURL)
        case let image as Markdown.Image:
            return styled(image.plainText, intent: intent, link: link)
        case is LineBreak:
            return AttributedString("\n")
        case is SoftBreak:
            return AttributedString(" ")
        case let html as InlineHTML:
            return styled(html.rawHTML, intent: intent, link: link)
        case let inline as InlineMarkup:
            return styled(inline.plainText, intent: intent, link: link)
        default:
            return AttributedString()
        }
    }

    private static func styled(_ string: String, intent: InlinePresentationIntent, link: URL?) -> AttributedString {
        var attributed = AttributedString(string)
        if !intent.isEmpty { attributed.inlinePresentationIntent = intent }
        if let link { attributed.link = link }
        return attributed
    }

    private static func resolveLink(_ destination: String, baseURL: URL?) -> URL? {
        if let direct = URL(string: destination), direct.scheme != nil { return direct }
        guard let baseURL else { return URL(string: destination) }
        return URL(string: destination, relativeTo: baseURL)
    }
}
