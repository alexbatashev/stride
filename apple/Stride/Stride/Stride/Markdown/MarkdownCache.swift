import Foundation

/// Memoizes Markdown parsing so scrolling a long conversation or streaming a
/// reply does not re-parse unchanged text.
///
/// `MarkdownView.body` runs every time SwiftUI re-evaluates a row, which happens
/// constantly while a `LazyVStack` recycles rows during a scroll. Without a cache
/// each pass re-ran the full CommonMark parser on the main thread — that cost is
/// what made scrolling stutter.
enum MarkdownCache {
    static func blocks(for text: String, baseURL: URL?) -> [MarkdownBlock] {
        let key = "\(baseURL?.absoluteString ?? "")\u{1}\(text)" as NSString
        if let cached = cache.object(forKey: key) { return cached.blocks }
        let parsed = MarkdownParser.parse(text, baseURL: baseURL)
        cache.setObject(Box(parsed), forKey: key)
        return parsed
    }

    private final class Box {
        let blocks: [MarkdownBlock]
        init(_ blocks: [MarkdownBlock]) { self.blocks = blocks }
    }

    private static let cache: NSCache<NSString, Box> = {
        let cache = NSCache<NSString, Box>()
        cache.countLimit = 512
        return cache
    }()
}
