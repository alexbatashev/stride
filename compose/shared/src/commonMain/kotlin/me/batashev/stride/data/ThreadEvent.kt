package me.batashev.stride.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/**
 * A single frame from the thread event WebSocket (`GET /api/threads/{id}/events`).
 * The server tags each `kind` with a `type` discriminator (kotlinx-serialization's
 * default class discriminator), so the polymorphic [EventKind] decodes natively.
 */
@Serializable
data class ThreadEvent(
    val seq: Long,
    @SerialName("thread_id") val threadId: String,
    @SerialName("run_id") val runId: String? = null,
    val kind: EventKind,
)

@Serializable
data class SnapshotMessage(
    @SerialName("run_id") val runId: String,
    val content: String,
    val thinking: String? = null,
)

@Serializable
data class PendingApproval(
    @SerialName("approval_id") val approvalId: String,
    val message: String,
)

@Serializable
data class PendingQuiz(
    @SerialName("quiz_id") val quizId: String,
    val questions: List<QuizQuestion>,
)

@Serializable
sealed interface EventKind {
    @Serializable
    @SerialName("Snapshot")
    data class Snapshot(
        val status: String? = null,
        @SerialName("in_progress") val inProgress: SnapshotMessage? = null,
        @SerialName("pending_approval") val pendingApproval: PendingApproval? = null,
        @SerialName("pending_quiz") val pendingQuiz: PendingQuiz? = null,
    ) : EventKind

    @Serializable @SerialName("RunStarted") data object RunStarted : EventKind

    @Serializable
    @SerialName("UserMessageCommitted")
    data class UserMessageCommitted(@SerialName("message_id") val messageId: String, val seq: Long) : EventKind

    @Serializable @SerialName("AgentDelta") data class AgentDelta(val content: String) : EventKind

    @Serializable @SerialName("ThinkingDelta") data class ThinkingDelta(val thinking: String) : EventKind

    @Serializable
    @SerialName("AgentMessageCommitted")
    data class AgentMessageCommitted(@SerialName("message_id") val messageId: String, val seq: Long) : EventKind

    @Serializable @SerialName("ToolStarted") data class ToolStarted(val name: String) : EventKind

    @Serializable @SerialName("ToolFinished") data class ToolFinished(val name: String) : EventKind

    @Serializable
    @SerialName("WaitingForApproval")
    data class WaitingForApproval(@SerialName("approval_id") val approvalId: String, val message: String) : EventKind

    @Serializable
    @SerialName("ApprovalResolved")
    data class ApprovalResolved(@SerialName("approval_id") val approvalId: String, val approved: Boolean) : EventKind

    @Serializable
    @SerialName("WaitingForQuiz")
    data class WaitingForQuiz(@SerialName("quiz_id") val quizId: String, val questions: List<QuizQuestion>) : EventKind

    @Serializable @SerialName("QuizAnswered") data class QuizAnswered(@SerialName("quiz_id") val quizId: String) : EventKind

    @Serializable @SerialName("RunFinished") data object RunFinished : EventKind

    @Serializable @SerialName("RunFailed") data class RunFailed(val error: String) : EventKind

    @Serializable @SerialName("RunCancelled") data object RunCancelled : EventKind
}
