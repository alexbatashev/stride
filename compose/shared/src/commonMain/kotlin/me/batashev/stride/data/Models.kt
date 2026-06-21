package me.batashev.stride.data

import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable

/** A conversation grouping. Mirrors the server `projects` table. */
@Serializable
data class Project(val id: String, val title: String)

/** One row in the thread list. Mirrors `GET /api/threads`. */
@Serializable
data class ThreadSummary(
    val id: String,
    val title: String,
    @SerialName("project_id") val projectId: String? = null,
)

/** Roles a stored message can carry. */
@Serializable
enum class MessageRole {
    @SerialName("system") System,
    @SerialName("agent") Agent,
    @SerialName("user") User,
    @SerialName("tool") Tool,
}

/** A persisted message. Mirrors `GET /api/threads/{id}/messages`. */
@Serializable
data class Message(
    val id: String,
    val seq: Long,
    val role: MessageRole,
    val content: String,
    val thinking: String? = null,
    @SerialName("tool_call_name") val toolCallName: String? = null,
)

/** Response from creating a thread or sending a message. */
@Serializable
data class SendResult(
    @SerialName("thread_id") val threadId: String,
    @SerialName("run_id") val runId: String,
)

/** A multiple-choice question the agent asks mid-run. */
@Serializable
data class QuizQuestion(val question: String, val options: List<String>)
