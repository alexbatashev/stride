import ComposableArchitecture
import SwiftUI

struct ChatView: View {
    @Bindable var store: StoreOf<ChatFeature>

    private let bottomID = "stride.chat.bottom"
    private var baseURL: URL? { Session.shared.baseURL }

    var body: some View {
        ZStack(alignment: .bottom) {
            messages
            bottomBar
        }
        .navigationTitle(store.title)
        #if os(iOS) || os(visionOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            if store.running {
                ToolbarItem(placement: .primaryAction) {
                    Button(role: .cancel) {
                        store.send(.cancelTapped)
                    } label: {
                        Label("Stop", systemImage: "stop.circle")
                    }
                }
            }
            if store.threadID != nil {
                ToolbarItem(placement: .automatic) {
                    Button {
                        store.send(.filesButtonTapped)
                    } label: {
                        Label("Files", systemImage: "folder")
                    }
                }
            }
        }
        .inspector(isPresented: $store.showFiles) {
            Group {
                if let filesStore = store.scope(state: \.files, action: \.files) {
                    FilesView(store: filesStore)
                } else {
                    ContentUnavailableView("Files", systemImage: "folder")
                }
            }
            .inspectorColumnWidth(min: 280, ideal: 340, max: 460)
        }
        .onAppear { store.send(.onAppear) }
    }

    private var messages: some View {
        ScrollViewReader { proxy in
            ScrollView {
                LazyVStack(alignment: .leading, spacing: Metrics.messageSpacing) {
                    if store.isLoadingHistory {
                        ProgressView()
                            .frame(maxWidth: .infinity)
                            .padding(.top, 40)
                    }

                    ForEach(store.messages) { message in
                        MessageRow(message: message, baseURL: baseURL)
                            .equatable()
                    }

                    if let streaming = store.streaming {
                        StreamingRow(streaming: streaming, baseURL: baseURL)
                    }

                    if let tool = store.activeTool {
                        ToolActivityRow(name: tool)
                    } else if store.running, store.streaming == nil {
                        TypingIndicator()
                    }

                    Color.clear
                        .frame(height: 1)
                        .id(bottomID)
                }
                .frame(maxWidth: Metrics.maxReadingWidth, alignment: .leading)
                .frame(maxWidth: .infinity)
                .padding(.horizontal, Metrics.gutter)
                .padding(.top, Metrics.gutter)
                .padding(.bottom, 130)
            }
            .scrollDismissesKeyboard(.interactively)
            .overlay {
                if store.messages.isEmpty, store.streaming == nil, !store.isLoadingHistory {
                    NewConversationHint()
                }
            }
            .onChange(of: store.messages.count) { _, _ in scrollToBottom(proxy) }
            .onChange(of: store.streaming?.content) { _, _ in scrollToBottom(proxy, animated: false) }
            .onChange(of: store.activeTool) { _, _ in scrollToBottom(proxy) }
            .onAppear { scrollToBottom(proxy, animated: false) }
        }
    }

    @ViewBuilder
    private var bottomBar: some View {
        VStack(spacing: 10) {
            if let error = store.errorMessage {
                ErrorBanner(text: error) { store.send(.dismissError) }
            }

            if let approval = store.pendingApproval {
                ApprovalCard(
                    message: approval.message,
                    onApprove: { store.send(.approvalResponse(true)) },
                    onDeny: { store.send(.approvalResponse(false)) }
                )
            } else if let quiz = store.pendingQuiz, let question = quiz.current {
                QuizCard(
                    question: question.question,
                    options: question.options,
                    progress: "\(quiz.index + 1) of \(quiz.questions.count)"
                ) { store.send(.quizSelected($0)) }
            } else {
                Composer(
                    text: $store.composer,
                    running: store.running,
                    canSend: store.canSend,
                    onSend: { store.send(.sendTapped) },
                    onStop: { store.send(.cancelTapped) }
                )
            }
        }
        .frame(maxWidth: Metrics.maxReadingWidth)
        .frame(maxWidth: .infinity)
        .padding(.horizontal, Metrics.gutter)
        .padding(.bottom, 10)
        .animation(.snappy, value: store.pendingApproval)
        .animation(.snappy, value: store.pendingQuiz)
        .animation(.snappy, value: store.errorMessage)
    }

    private func scrollToBottom(_ proxy: ScrollViewProxy, animated: Bool = true) {
        if animated {
            withAnimation(.easeOut(duration: 0.2)) {
                proxy.scrollTo(bottomID, anchor: .bottom)
            }
        } else {
            proxy.scrollTo(bottomID, anchor: .bottom)
        }
    }
}

/// Subtle prompt shown for a brand-new, empty conversation.
private struct NewConversationHint: View {
    var body: some View {
        VStack(spacing: 12) {
            Image(systemName: "sparkles")
                .font(.system(size: 40, weight: .light))
                .foregroundStyle(.tint)
            Text("What are we working on?")
                .font(.title3.weight(.semibold))
            Text("Ask S.T.R.I.D.E. anything to get started.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding(32)
    }
}
