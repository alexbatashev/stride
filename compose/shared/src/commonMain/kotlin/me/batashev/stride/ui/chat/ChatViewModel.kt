package me.batashev.stride.ui.chat

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import me.batashev.stride.data.EventKind
import me.batashev.stride.data.FridayClient
import me.batashev.stride.data.FridayException
import me.batashev.stride.data.Message
import me.batashev.stride.data.MessageRole
import me.batashev.stride.data.QuizQuestion
import me.batashev.stride.data.ThreadEvent

/**
 * Drives one conversation. Streaming text lives in [UiState.streaming] until the
 * server commits it, at which point the authoritative history is reloaded. Ports
 * the reducer logic from the Apple app's `ChatFeature`.
 */
class ChatViewModel(
    private val client: FridayClient,
    initialThreadId: String?,
    private val onThreadsChanged: () -> Unit,
    private val onUnauthorized: () -> Unit,
) : ViewModel() {

    data class ChatMessage(
        val id: String,
        val seq: Long,
        val role: MessageRole,
        val content: String,
        val thinking: String? = null,
        val toolName: String? = null,
        val pending: Boolean = false,
    )

    data class Streaming(val content: String = "", val thinking: String = "")

    data class Approval(val id: String, val message: String)

    data class Quiz(
        val id: String,
        val questions: List<QuizQuestion>,
        val index: Int = 0,
        val answers: List<String> = emptyList(),
    ) {
        val current: QuizQuestion? get() = questions.getOrNull(index)
        val progress: String get() = "${index + 1} of ${questions.size}"
    }

    data class UiState(
        val threadId: String? = null,
        val messages: List<ChatMessage> = emptyList(),
        val streaming: Streaming? = null,
        val running: Boolean = false,
        val activeTool: String? = null,
        val approval: Approval? = null,
        val quiz: Quiz? = null,
        val error: String? = null,
        val loadingHistory: Boolean = false,
    ) {
        val isNewThread: Boolean get() = threadId == null
        fun canSend(draft: String): Boolean =
            draft.isNotBlank() && !running && approval == null && quiz == null
    }

    private val _state = MutableStateFlow(
        UiState(threadId = initialThreadId, loadingHistory = initialThreadId != null),
    )
    val state: StateFlow<UiState> = _state.asStateFlow()

    private var lastEventSeq = -1L
    private var eventsJob: Job? = null

    init {
        if (initialThreadId != null) {
            reloadHistory()
            connect()
        }
    }

    fun send(draft: String) {
        val content = draft.trim()
        if (content.isEmpty() || _state.value.running) return
        _state.update {
            it.copy(error = null, running = true, messages = it.messages + pendingUser(content, it.messages.size))
        }
        val threadId = _state.value.threadId
        viewModelScope.launch {
            try {
                if (threadId != null) {
                    client.sendMessage(threadId, content, emptyList())
                } else {
                    val result = client.createThread(content, null, emptyList())
                    lastEventSeq = -1
                    _state.update { it.copy(threadId = result.threadId) }
                    onThreadsChanged()
                    connect()
                    reloadHistory()
                }
            } catch (e: FridayException.Unauthorized) {
                onUnauthorized()
            } catch (e: Throwable) {
                failSend()
            }
        }
    }

    fun cancel() {
        val threadId = _state.value.threadId ?: return
        if (!_state.value.running) return
        viewModelScope.launch { runCatching { client.cancelRun(threadId) } }
    }

    fun resolveApproval(approved: Boolean) {
        val threadId = _state.value.threadId ?: return
        val approval = _state.value.approval ?: return
        _state.update { it.copy(approval = null) }
        viewModelScope.launch {
            try {
                client.resolveApproval(threadId, approval.id, approved)
            } catch (e: Throwable) {
                _state.update { it.copy(approval = approval, error = "Couldn't send your response. Try again.") }
            }
        }
    }

    fun answerQuiz(answer: String) {
        val threadId = _state.value.threadId ?: return
        val quiz = _state.value.quiz ?: return
        val answers = quiz.answers + answer
        if (quiz.index + 1 < quiz.questions.size) {
            _state.update { it.copy(quiz = quiz.copy(index = quiz.index + 1, answers = answers)) }
            return
        }
        _state.update { it.copy(quiz = null) }
        viewModelScope.launch {
            try {
                client.answerQuiz(threadId, quiz.id, answers)
            } catch (e: Throwable) {
                _state.update { it.copy(quiz = quiz.copy(answers = answers), error = "Couldn't send your answers. Try again.") }
            }
        }
    }

    fun dismissError() = _state.update { it.copy(error = null) }

    private fun connect() {
        val threadId = _state.value.threadId ?: return
        eventsJob?.cancel()
        eventsJob = viewModelScope.launch {
            while (isActive) {
                try {
                    client.events(threadId).collect { onEvent(it) }
                } catch (_: Throwable) {
                }
                if (!isActive) break
                delay(RECONNECT_DELAY_MS)
            }
        }
    }

    private fun reloadHistory() {
        val threadId = _state.value.threadId ?: return
        viewModelScope.launch {
            try {
                val messages = client.listMessages(threadId)
                _state.update {
                    it.copy(loadingHistory = false, streaming = null, messages = messages.map(::toChatMessage))
                }
            } catch (e: FridayException.Unauthorized) {
                onUnauthorized()
            } catch (e: Throwable) {
                _state.update { it.copy(loadingHistory = false) }
            }
        }
    }

    private fun onEvent(event: ThreadEvent) {
        val threadId = _state.value.threadId ?: return
        if (event.threadId != threadId) return

        if (event.kind is EventKind.Snapshot) {
            lastEventSeq = event.seq
        } else {
            if (event.seq <= lastEventSeq) return
            lastEventSeq = event.seq
        }

        when (val kind = event.kind) {
            is EventKind.Snapshot -> _state.update {
                it.copy(
                    running = kind.status == "running",
                    approval = kind.pendingApproval?.let { a -> Approval(a.approvalId, a.message) },
                    quiz = kind.pendingQuiz?.takeIf { q -> q.questions.isNotEmpty() }
                        ?.let { q -> Quiz(q.quizId, q.questions) },
                    streaming = kind.inProgress?.takeIf { ip -> ip.content.isNotEmpty() }
                        ?.let { ip -> Streaming(ip.content, ip.thinking ?: "") } ?: it.streaming,
                )
            }

            EventKind.RunStarted -> _state.update { it.copy(running = true) }

            is EventKind.UserMessageCommitted -> Unit

            is EventKind.AgentDelta -> _state.update {
                val streaming = it.streaming ?: Streaming()
                it.copy(running = true, streaming = streaming.copy(content = streaming.content + kind.content))
            }

            is EventKind.ThinkingDelta -> _state.update {
                val streaming = it.streaming ?: Streaming()
                it.copy(streaming = streaming.copy(thinking = streaming.thinking + kind.thinking))
            }

            is EventKind.AgentMessageCommitted -> {
                _state.update { it.copy(streaming = null) }
                reloadHistory()
            }

            is EventKind.ToolStarted -> _state.update { it.copy(running = true, activeTool = kind.name) }

            is EventKind.ToolFinished -> {
                _state.update { it.copy(activeTool = null, approval = null, quiz = null) }
                reloadHistory()
            }

            is EventKind.WaitingForApproval -> _state.update {
                it.copy(running = true, approval = Approval(kind.approvalId, kind.message))
            }

            is EventKind.ApprovalResolved -> _state.update {
                if (it.approval?.id == kind.approvalId) it.copy(approval = null) else it
            }

            is EventKind.WaitingForQuiz -> _state.update {
                it.copy(running = true, quiz = Quiz(kind.quizId, kind.questions))
            }

            is EventKind.QuizAnswered -> _state.update {
                if (it.quiz?.id == kind.quizId) it.copy(quiz = null) else it
            }

            EventKind.RunFinished -> {
                resetRun()
                reloadHistory()
                onThreadsChanged()
            }

            is EventKind.RunFailed -> {
                resetRun()
                _state.update { it.copy(error = kind.error) }
            }

            EventKind.RunCancelled -> {
                resetRun()
                reloadHistory()
            }
        }
    }

    private fun resetRun() = _state.update {
        it.copy(running = false, activeTool = null, approval = null, quiz = null, streaming = null)
    }

    private fun failSend() = _state.update {
        it.copy(
            running = false,
            messages = it.messages.filterNot { m -> m.pending && m.role == MessageRole.User },
            error = "Couldn't send your message.",
        )
    }

    private fun pendingUser(content: String, index: Int) = ChatMessage(
        id = "pending-user-$index",
        seq = Long.MAX_VALUE,
        role = MessageRole.User,
        content = content,
        pending = true,
    )

    private fun toChatMessage(message: Message) = ChatMessage(
        id = message.id,
        seq = message.seq,
        role = message.role,
        content = message.content,
        thinking = message.thinking,
        toolName = message.toolCallName,
    )

    private companion object {
        const val RECONNECT_DELAY_MS = 2000L
    }
}
