import Foundation

/// A block-level element produced by ``MarkdownParser``.
enum MarkdownBlock: Equatable, Identifiable {
    case heading(level: Int, text: String)
    case paragraph(text: String)
    case bulletList(items: [String])
    case orderedList(items: [OrderedItem])
    case quote(text: String)
    case code(language: String?, code: String)
    case image(alt: String, url: String)
    case rule

    struct OrderedItem: Equatable, Identifiable {
        let id = UUID()
        let number: Int
        let text: String
    }

    var id: String {
        switch self {
        case .heading(let level, let text): return "h\(level):\(text)"
        case .paragraph(let text): return "p:\(text)"
        case .bulletList(let items): return "ul:\(items.joined(separator: "|"))"
        case .orderedList(let items): return "ol:\(items.map { "\($0.number).\($0.text)" }.joined(separator: "|"))"
        case .quote(let text): return "q:\(text)"
        case .code(let language, let code): return "code:\(language ?? "")\n\(code)"
        case .image(let alt, let url): return "img:\(url):\(alt)"
        case .rule: return "hr"
        }
    }
}

/// A line-based, fault-tolerant Markdown block parser. It is deliberately lenient
/// so that text still streaming from the agent (an unclosed code fence, a dangling
/// emphasis marker) renders sensibly mid-flight.
enum MarkdownParser {
    static func parse(_ source: String) -> [MarkdownBlock] {
        var blocks: [MarkdownBlock] = []
        let lines = source.replacingOccurrences(of: "\r\n", with: "\n").components(separatedBy: "\n")
        var index = 0

        while index < lines.count {
            let line = lines[index]
            let trimmed = line.trimmingCharacters(in: .whitespaces)

            if trimmed.isEmpty {
                index += 1
                continue
            }

            if let fence = fenceLanguage(trimmed) {
                let (block, next) = consumeCodeBlock(lines, start: index + 1, language: fence)
                blocks.append(block)
                index = next
                continue
            }

            if isRule(trimmed) {
                blocks.append(.rule)
                index += 1
                continue
            }

            if let heading = heading(trimmed) {
                blocks.append(heading)
                index += 1
                continue
            }

            if let image = standaloneImage(trimmed) {
                blocks.append(image)
                index += 1
                continue
            }

            if trimmed.hasPrefix(">") {
                let (block, next) = consumeQuote(lines, start: index)
                blocks.append(block)
                index = next
                continue
            }

            if isBullet(trimmed) {
                let (block, next) = consumeBulletList(lines, start: index)
                blocks.append(block)
                index = next
                continue
            }

            if orderedPrefix(trimmed) != nil {
                let (block, next) = consumeOrderedList(lines, start: index)
                blocks.append(block)
                index = next
                continue
            }

            let (block, next) = consumeParagraph(lines, start: index)
            blocks.append(block)
            index = next
        }

        return blocks
    }

    // MARK: - Block detectors

    private static func fenceLanguage(_ trimmed: String) -> String? {
        guard trimmed.hasPrefix("```") else { return nil }
        let language = trimmed.dropFirst(3).trimmingCharacters(in: .whitespaces)
        return language.isEmpty ? "" : language
    }

    private static func isRule(_ trimmed: String) -> Bool {
        let stripped = trimmed.filter { !$0.isWhitespace }
        guard stripped.count >= 3 else { return false }
        return stripped.allSatisfy { $0 == "-" } || stripped.allSatisfy { $0 == "*" } || stripped.allSatisfy { $0 == "_" }
    }

    private static func heading(_ trimmed: String) -> MarkdownBlock? {
        var level = 0
        for char in trimmed {
            if char == "#" { level += 1 } else { break }
        }
        guard level >= 1, level <= 6 else { return nil }
        let rest = trimmed.dropFirst(level)
        guard rest.first == " " else { return nil }
        return .heading(level: level, text: rest.trimmingCharacters(in: .whitespaces))
    }

    private static func standaloneImage(_ trimmed: String) -> MarkdownBlock? {
        guard trimmed.hasPrefix("!["), trimmed.hasSuffix(")"),
              let bracket = trimmed.firstIndex(of: "]"),
              let paren = trimmed.firstIndex(of: "(")
        else { return nil }
        let alt = String(trimmed[trimmed.index(trimmed.startIndex, offsetBy: 2)..<bracket])
        let url = String(trimmed[trimmed.index(after: paren)..<trimmed.index(before: trimmed.endIndex)])
        return .image(alt: alt, url: url)
    }

    private static func isBullet(_ trimmed: String) -> Bool {
        trimmed.hasPrefix("- ") || trimmed.hasPrefix("* ") || trimmed.hasPrefix("+ ")
    }

    private static func orderedPrefix(_ trimmed: String) -> Int? {
        let digits = trimmed.prefix { $0.isNumber }
        guard !digits.isEmpty, let number = Int(digits) else { return nil }
        let afterDigits = trimmed.dropFirst(digits.count)
        guard afterDigits.first == ".", afterDigits.dropFirst().first == " " else { return nil }
        return number
    }

    // MARK: - Block consumers

    private static func consumeCodeBlock(_ lines: [String], start: Int, language: String) -> (MarkdownBlock, Int) {
        var body: [String] = []
        var index = start
        while index < lines.count {
            if lines[index].trimmingCharacters(in: .whitespaces) == "```" {
                index += 1
                break
            }
            body.append(lines[index])
            index += 1
        }
        let lang = language.isEmpty ? nil : language
        return (.code(language: lang, code: body.joined(separator: "\n")), index)
    }

    private static func consumeQuote(_ lines: [String], start: Int) -> (MarkdownBlock, Int) {
        var body: [String] = []
        var index = start
        while index < lines.count, lines[index].trimmingCharacters(in: .whitespaces).hasPrefix(">") {
            let trimmed = lines[index].trimmingCharacters(in: .whitespaces)
            body.append(String(trimmed.dropFirst()).trimmingCharacters(in: .whitespaces))
            index += 1
        }
        return (.quote(text: body.joined(separator: "\n")), index)
    }

    private static func consumeBulletList(_ lines: [String], start: Int) -> (MarkdownBlock, Int) {
        var items: [String] = []
        var index = start
        while index < lines.count {
            let trimmed = lines[index].trimmingCharacters(in: .whitespaces)
            guard isBullet(trimmed) else { break }
            items.append(String(trimmed.dropFirst(2)))
            index += 1
        }
        return (.bulletList(items: items), index)
    }

    private static func consumeOrderedList(_ lines: [String], start: Int) -> (MarkdownBlock, Int) {
        var items: [MarkdownBlock.OrderedItem] = []
        var index = start
        while index < lines.count {
            let trimmed = lines[index].trimmingCharacters(in: .whitespaces)
            guard let number = orderedPrefix(trimmed) else { break }
            let text = trimmed.drop { $0.isNumber }.dropFirst(2)
            items.append(.init(number: number, text: String(text)))
            index += 1
        }
        return (.orderedList(items: items), index)
    }

    private static func consumeParagraph(_ lines: [String], start: Int) -> (MarkdownBlock, Int) {
        var body: [String] = []
        var index = start
        while index < lines.count {
            let trimmed = lines[index].trimmingCharacters(in: .whitespaces)
            if trimmed.isEmpty || trimmed.hasPrefix("```") || trimmed.hasPrefix(">")
                || isBullet(trimmed) || orderedPrefix(trimmed) != nil || heading(trimmed) != nil
                || isRule(trimmed) {
                break
            }
            body.append(trimmed)
            index += 1
        }
        return (.paragraph(text: body.joined(separator: "\n")), index)
    }
}
