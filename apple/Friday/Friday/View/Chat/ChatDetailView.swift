import CoreFriday
import Markdown
import SwiftUI

#if os(macOS)
    import AppKit
#elseif canImport(UIKit)
    import UIKit
#endif

struct ChatDetailView: View {
    @Environment(ModelData.self) private var modelData
    let thread: ChatThread

    @State private var messages: [ChatMessage] = []
    @State private var errorMessageIDs: Set<String> = []
    @State private var draftText: String = ""
    @State private var isSending = false
    @State private var currentStreamTask: Task<Void, Never>?

    @State private var showingModelPopover = false

    var body: some View {
        ZStack(alignment: .bottom) {
            // Transcript scrolls behind the composer
            transcript

            // Composer floats on top with blur background
            composer
        }
        .task(id: thread.id) {
            messages = await modelData.getMessages(for: thread)
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
                Button {
                } label: {
                    Label("Share", systemImage: "square.and.arrow.up")
                }
            }
        }
        .toolbarRole(.editor)
    }

    private var currentModelDisplayName: String {
        if let provider = modelData.chatSettings.activeProvider,
            !modelData.chatSettings.activeModel.isEmpty
        {
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
                provider: provider.id,
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
                    ForEach(Array(availableModels.prefix(3).enumerated()), id: \.element.model) {
                        index, langModel in
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

    private func cancelCurrentRequest() {
        currentStreamTask?.cancel()
        currentStreamTask = nil
        isSending = false
    }

    @State private var scrollProxy: ScrollViewProxy?

    private var transcript: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(spacing: 12) {
                    ForEach(messages) { message in
                        TurnBubble(
                            message: message,
                            isError: errorMessageIDs.contains(message.id)
                        )
                        .id(message.id)
                    }

                    // Invisible anchor at the bottom
                    Color.clear
                        .frame(height: 1)
                        .id("bottom")
                }
                .padding(.horizontal, 16)
                .padding(.top, 18)
                .padding(.bottom, 120)  // Extra padding so content can scroll behind the composer
            }
            .onAppear {
                scrollProxy = proxy
                proxy.scrollTo("bottom", anchor: .bottom)
            }
            .onChange(of: messages.count) { _, _ in
                withAnimation(.easeOut(duration: 0.2)) {
                    proxy.scrollTo("bottom", anchor: .bottom)
                }
            }
        }
    }

    private var composer: some View {
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
                    }
                    .padding(.horizontal, 12)
                    .padding(.vertical, 8)
                    .glassEffect(
                        .regular.interactive(), in: .rect(cornerRadius: 20, style: .continuous))

                    Group {
                        if isSending {
                            Button {
                                cancelCurrentRequest()
                            } label: {
                                ZStack {
                                    Image(systemName: "stop.fill")
                                        .font(.system(size: 14, weight: .bold))
                                        .foregroundStyle(.white)
                                }
                                .frame(width: 30, height: 30)
                                .glassEffect(.regular.tint(.red).interactive(), in: .circle)
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("stopMessageButton")
                            .help("Stop generation")
                            .transition(.opacity.combined(with: .scale))
                        } else if draftText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        {
                            Button {
                                // TODO: Implement voice dictation
                            } label: {
                                Image(systemName: "waveform")
                                    .font(.system(size: 17, weight: .medium))
                                    .frame(width: 30, height: 30)
                                    .glassEffect(.regular.interactive(), in: .circle)
                            }
                            .buttonStyle(.plain)
                            .help("Voice input")
                            .transition(.opacity.combined(with: .scale))
                        } else {
                            Button(action: sendMessage) {
                                Image(systemName: "arrow.up")
                                    .font(.system(size: 16, weight: .bold))
                                    .frame(width: 30, height: 30)
                                    .glassEffect(
                                        .regular.tint(.accentColor).interactive(), in: .circle)
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("sendMessageButton")
                            .help("Send message")
                            .transition(.opacity.combined(with: .scale))
                        }
                    }
                    .animation(.spring(duration: 0.2), value: draftText.isEmpty)
                    .animation(.spring(duration: 0.2), value: isSending)
                }
                .padding(.horizontal, 12)
                .padding(.vertical, 8)
            }
        }
    }

    private func sendMessage() {
        let trimmed = draftText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        let userMessage = ChatMessage.makeNew(threadId: thread.id, role: .user, content: trimmed)
        let assistantMessage = ChatMessage.makeNew(
            threadId: thread.id, role: .assistant, content: "")
        let assistantId = assistantMessage.id

        messages.append(userMessage)
        messages.append(assistantMessage)

        draftText = ""
        isSending = true

        if let scrollProxy {
            withAnimation(.easeOut(duration: 0.3)) {
                scrollProxy.scrollTo("bottom", anchor: .bottom)
            }
        }

        currentStreamTask = Task {
            defer {
                Task { @MainActor in
                    isSending = false
                    currentStreamTask = nil
                }
            }

            do {
                for try await partial in await modelData.addMessage(text: trimmed, in: thread) {
                    if Task.isCancelled {
                        await MainActor.run {
                            if let idx = messages.firstIndex(where: { $0.id == assistantId }) {
                                let current = messages[idx].content.trimmingCharacters(
                                    in: .whitespacesAndNewlines)
                                let updated =
                                    current.isEmpty
                                    ? "(Generation cancelled)"
                                    : current + "\n\n(Generation cancelled)"
                                messages[idx] = messages[idx].withContent(updated)
                                modelData.updateThread(thread.id, previewText: updated)
                            }
                            errorMessageIDs.insert(assistantId)
                        }
                        return
                    }

                    await MainActor.run {
                        if let idx = messages.firstIndex(where: { $0.id == assistantId }) {
                            messages[idx] = messages[idx].withContent(partial.content)
                        }
                        modelData.updateThread(thread.id, previewText: partial.content)
                    }
                }

                await MainActor.run {
                    if let idx = messages.firstIndex(where: { $0.id == assistantId }) {
                        let content = messages[idx].content
                        if content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                            messages[idx] = messages[idx].withContent("(No text returned)")
                        }
                        modelData.updateThread(thread.id, previewText: messages[idx].content)
                    }
                }
            } catch {
                await MainActor.run {
                    if let idx = messages.firstIndex(where: { $0.id == assistantId }) {
                        let base = messages[idx].content.trimmingCharacters(
                            in: .whitespacesAndNewlines)
                        let updated =
                            base.isEmpty
                            ? "Request failed: \(error.localizedDescription)"
                            : base + "\n\nRequest failed: \(error.localizedDescription)"
                        messages[idx] = messages[idx].withContent(updated)
                        modelData.updateThread(thread.id, previewText: updated)
                    }
                    errorMessageIDs.insert(assistantId)
                }
            }
        }
    }
}

private struct TurnBubble: View {
    let message: ChatMessage
    let isError: Bool

    private var isUser: Bool { message.role == .user }

    var body: some View {
        HStack(alignment: .bottom) {
            if isUser { Spacer(minLength: 44) }

            VStack(alignment: .leading, spacing: 8) {
                if isUser {
                    Text(message.content)
                        .textSelection(.enabled)
                } else {
                    AssistantMarkdownView(
                        markdown: message.content,
                        onCopyMarkdown: { copyPlainTextToPasteboard(message.content) },
                        onCopyRichText: { copyRichTextToPasteboard(markdown: message.content) }
                    )
                }

                Text(message.createdAt, style: .time)
                    .font(.caption2)
                    .foregroundStyle(.secondary)
            }
            .padding(.horizontal, 14)
            .padding(.vertical, 12)
            .background(bubbleBackground)
            .clipShape(RoundedRectangle(cornerRadius: 18, style: .continuous))
            .contextMenu {
                if !isUser {
                    Button("Copy Reply as Markdown") {
                        copyPlainTextToPasteboard(message.content)
                    }

                    Button("Copy Reply as Rich Text") {
                        copyRichTextToPasteboard(markdown: message.content)
                    }
                }
            }

            if !isUser { Spacer(minLength: 44) }
        }
    }

    private var bubbleBackground: some ShapeStyle {
        if isError {
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

    private func copyPlainTextToPasteboard(_ text: String) {
        #if os(macOS)
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            pasteboard.setString(text, forType: .string)
        #elseif canImport(UIKit)
            UIPasteboard.general.string = text
        #endif
    }

    private func copyRichTextToPasteboard(markdown: String) {
        let attributed = attributedMarkdownForWholeReply(markdown)
        let nsAttributed = NSAttributedString(attributed)

        #if os(macOS)
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            if let rtf = try? nsAttributed.data(
                from: NSRange(location: 0, length: nsAttributed.length),
                documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf]
            ) {
                pasteboard.setData(rtf, forType: .rtf)
            }
            pasteboard.setString(nsAttributed.string, forType: .string)
        #elseif canImport(UIKit)
            if let rtf = try? nsAttributed.data(
                from: NSRange(location: 0, length: nsAttributed.length),
                documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf]
            ) {
                UIPasteboard.general.setData(rtf, forPasteboardType: "public.rtf")
            }
            UIPasteboard.general.string = nsAttributed.string
        #endif
    }
}

private struct AssistantMarkdownView: View {
    let markdown: String
    let onCopyMarkdown: () -> Void
    let onCopyRichText: () -> Void

    private var document: Document {
        Document(parsing: markdown)
    }

    private enum Chunk {
        case richText(AttributedString)
        case table(Markdown.Table)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 10) {
            ForEach(Array(chunks.enumerated()), id: \.offset) { _, chunk in
                switch chunk {
                case .table(let table):
                    MarkdownTableView(
                        table: table,
                        onCopyMarkdown: onCopyMarkdown,
                        onCopyRichText: onCopyRichText
                    )
                case .richText(let attributed):
                    richTextView(for: attributed)
                }
            }
        }
        .textSelection(.enabled)
    }

    @ViewBuilder
    private func richTextView(for attributed: AttributedString) -> some View {
        #if os(macOS)
            ReplySelectableTextView(
                displayedText: NSAttributedString(attributed),
                fullReplyMarkdown: markdown,
                fullReplyRichText: NSAttributedString(attributedMarkdownForWholeReply(markdown))
            )
        #else
            Text(attributed)
                .contextMenu {
                    Button("Copy Reply as Markdown") {
                        onCopyMarkdown()
                    }

                    Button("Copy Reply as Rich Text") {
                        onCopyRichText()
                    }
                }
        #endif
    }

    private var chunks: [Chunk] {
        var result: [Chunk] = []
        var pendingTextBlocks: [Markup] = []

        func flushPendingTextBlocks() {
            guard !pendingTextBlocks.isEmpty else { return }
            result.append(.richText(attributedMarkdown(for: pendingTextBlocks)))
            pendingTextBlocks.removeAll()
        }

        for block in document.children {
            if let table = block as? Markdown.Table {
                flushPendingTextBlocks()
                result.append(.table(table))
            } else {
                pendingTextBlocks.append(block)
            }
        }

        flushPendingTextBlocks()
        return result
    }

    private func attributedMarkdown(for markups: [Markup]) -> AttributedString {
        var combined = AttributedString()

        for (index, markup) in markups.enumerated() {
            if index > 0 {
                combined += AttributedString("\n\n")
            }
            combined += attributedMarkdown(for: markup)
        }

        return combined
    }

    private func attributedMarkdown(for markup: Markup) -> AttributedString {
        var rewriter = SoftBreakToHardBreakRewriter()
        let rewritten = rewriter.visit(markup) ?? markup
        let markdownSource = rewritten.format()
        return attributedFromMarkdownSource(markdownSource)
    }

    private func attributedFromMarkdownSource(_ markdownSource: String) -> AttributedString {
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .full,
            failurePolicy: .returnPartiallyParsedIfPossible
        )

        if let attributed = try? AttributedString(markdown: markdownSource, options: options) {
            return attributed
        }

        return AttributedString(markdownSource)
    }
}

private struct MarkdownTableView: View {
    let table: Markdown.Table
    let onCopyMarkdown: () -> Void
    let onCopyRichText: () -> Void

    private var headerCells: [Markdown.Table.Cell] {
        Array(table.head.cells)
    }

    private var rows: [Markdown.Table.Row] {
        Array(table.body.rows)
    }

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            VStack(alignment: .leading, spacing: 0) {
                Grid(alignment: .leading, horizontalSpacing: 0, verticalSpacing: 0) {
                    GridRow {
                        ForEach(Array(headerCells.enumerated()), id: \.offset) { index, cell in
                            cellView(cell, isHeader: true, column: index)
                        }
                    }

                    ForEach(Array(rows.enumerated()), id: \.offset) { _, row in
                        GridRow {
                            ForEach(Array(row.cells.enumerated()), id: \.offset) { index, cell in
                                cellView(cell, isHeader: false, column: index)
                            }
                        }
                    }
                }
                .background(.regularMaterial)
                .clipShape(RoundedRectangle(cornerRadius: 8, style: .continuous))
            }
        }
    }

    @ViewBuilder
    private func cellView(_ cell: Markdown.Table.Cell, isHeader: Bool, column: Int) -> some View {
        let text = cellAttributedMarkdown(cell)

        Text(text)
            .font(isHeader ? .subheadline.weight(.semibold) : .subheadline)
            .frame(minWidth: 84, maxWidth: .infinity, alignment: alignment(for: column))
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(isHeader ? Color.primary.opacity(0.08) : Color.clear)
            .overlay(alignment: .topLeading) {
                Rectangle()
                    .fill(Color.primary.opacity(0.14))
                    .frame(height: 0.5)
            }
            .overlay(alignment: .leading) {
                Rectangle()
                    .fill(Color.primary.opacity(0.14))
                    .frame(width: 0.5)
            }
            .contextMenu {
                Button("Copy Reply as Markdown") {
                    onCopyMarkdown()
                }

                Button("Copy Reply as Rich Text") {
                    onCopyRichText()
                }
            }
    }

    private func cellAttributedMarkdown(_ cell: Markdown.Table.Cell) -> AttributedString {
        let markdownSource = cellMarkdownSource(cell)
        let options = AttributedString.MarkdownParsingOptions(
            interpretedSyntax: .full,
            failurePolicy: .returnPartiallyParsedIfPossible
        )

        if let attributed = try? AttributedString(markdown: markdownSource, options: options) {
            return attributed
        }

        return AttributedString(markdownSource)
    }

    private func cellMarkdownSource(_ cell: Markdown.Table.Cell) -> String {
        cell.inlineChildren.map { inline in
            var rewriter = SoftBreakToHardBreakRewriter()
            let rewritten = rewriter.visit(inline) ?? inline
            return rewritten.format()
        }
        .joined()
    }

    private func alignment(for column: Int) -> Alignment {
        guard column < table.columnAlignments.count else {
            return .leading
        }

        switch table.columnAlignments[column] {
        case .left:
            return .leading
        case .center:
            return .center
        case .right:
            return .trailing
        case .none:
            return .leading
        }
    }
}

private struct SoftBreakToHardBreakRewriter: MarkupRewriter {
    mutating func visitSoftBreak(_ softBreak: SoftBreak) -> Markup? {
        LineBreak()
    }
}

#if os(macOS)
    private struct ReplySelectableTextView: NSViewRepresentable {
        let displayedText: NSAttributedString
        let fullReplyMarkdown: String
        let fullReplyRichText: NSAttributedString

        func makeNSView(context: Context) -> ReplyTextView {
            let view = ReplyTextView()
            view.isEditable = false
            view.isSelectable = true
            view.drawsBackground = false
            view.isRichText = true
            view.importsGraphics = false
            view.textContainerInset = NSSize(width: 0, height: 0)
            view.textContainer?.lineFragmentPadding = 0
            view.textContainer?.widthTracksTextView = true
            view.isHorizontallyResizable = false
            view.isVerticallyResizable = true
            view.maxSize = NSSize(
                width: CGFloat.greatestFiniteMagnitude, height: CGFloat.greatestFiniteMagnitude)
            view.setContentCompressionResistancePriority(.required, for: .vertical)
            view.setContentHuggingPriority(.required, for: .vertical)
            view.updateContextMenu()
            return view
        }

        func updateNSView(_ view: ReplyTextView, context: Context) {
            view.fullReplyMarkdown = fullReplyMarkdown
            view.fullReplyRichText = fullReplyRichText
            view.updateContextMenu()

            let display = displayedText.withDefaultTextColor(.labelColor)

            if view.textStorage?.string != display.string {
                view.textStorage?.setAttributedString(display)
                view.invalidateIntrinsicContentSize()
            } else if let storage = view.textStorage, storage.length != display.length {
                storage.setAttributedString(display)
                view.invalidateIntrinsicContentSize()
            }
        }
    }

    private final class ReplyTextView: NSTextView {
        var fullReplyMarkdown: String = ""
        var fullReplyRichText: NSAttributedString = NSAttributedString()

        override var intrinsicContentSize: NSSize {
            guard let layoutManager, let textContainer else {
                return super.intrinsicContentSize
            }
            layoutManager.ensureLayout(for: textContainer)
            let usedRect = layoutManager.usedRect(for: textContainer)
            let height = ceil(usedRect.height + textContainerInset.height * 2)
            return NSSize(width: NSView.noIntrinsicMetric, height: max(1, height))
        }

        override func setFrameSize(_ newSize: NSSize) {
            super.setFrameSize(newSize)
            invalidateIntrinsicContentSize()
        }

        func updateContextMenu() {
            let menu = NSMenu()
            let copyMarkdownItem = NSMenuItem(
                title: "Copy Reply as Markdown",
                action: #selector(copyReplyAsMarkdown),
                keyEquivalent: ""
            )
            copyMarkdownItem.target = self

            let copyRichItem = NSMenuItem(
                title: "Copy Reply as Rich Text",
                action: #selector(copyReplyAsRichText),
                keyEquivalent: ""
            )
            copyRichItem.target = self

            menu.addItem(copyMarkdownItem)
            menu.addItem(copyRichItem)
            menu.addItem(.separator())
            menu.addItem(withTitle: "Copy", action: #selector(copy(_:)), keyEquivalent: "")
            menu.addItem(
                withTitle: "Select All", action: #selector(selectAll(_:)), keyEquivalent: "")
            self.menu = menu
        }

        @objc private func copyReplyAsMarkdown() {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()
            pasteboard.setString(fullReplyMarkdown, forType: .string)
        }

        @objc private func copyReplyAsRichText() {
            let pasteboard = NSPasteboard.general
            pasteboard.clearContents()

            if let rtf = try? fullReplyRichText.data(
                from: NSRange(location: 0, length: fullReplyRichText.length),
                documentAttributes: [.documentType: NSAttributedString.DocumentType.rtf]
            ) {
                pasteboard.setData(rtf, forType: .rtf)
            }

            pasteboard.setString(fullReplyRichText.string, forType: .string)
        }
    }

    extension NSAttributedString {
        fileprivate func withDefaultTextColor(_ color: NSColor) -> NSAttributedString {
            guard length > 0 else { return self }

            let mutable = NSMutableAttributedString(attributedString: self)
            let fullRange = NSRange(location: 0, length: mutable.length)
            mutable.enumerateAttribute(.foregroundColor, in: fullRange, options: []) {
                value, range, _ in
                if value == nil {
                    mutable.addAttribute(.foregroundColor, value: color, range: range)
                }
            }
            return mutable
        }
    }
#endif

private func attributedMarkdownForWholeReply(_ markdown: String) -> AttributedString {
    let document = Document(parsing: markdown)
    var rewriter = SoftBreakToHardBreakRewriter()
    let rewritten = rewriter.visit(document) ?? document
    let markdownSource = rewritten.format()

    let options = AttributedString.MarkdownParsingOptions(
        interpretedSyntax: .full,
        failurePolicy: .returnPartiallyParsedIfPossible
    )

    if let attributed = try? AttributedString(markdown: markdownSource, options: options) {
        return attributed
    }

    return AttributedString(markdownSource)
}
