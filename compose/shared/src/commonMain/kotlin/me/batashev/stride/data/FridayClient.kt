package me.batashev.stride.data

import io.ktor.client.HttpClient
import io.ktor.client.call.body
import io.ktor.client.plugins.contentnegotiation.ContentNegotiation
import io.ktor.client.plugins.websocket.WebSockets
import io.ktor.client.plugins.websocket.webSocket
import io.ktor.client.request.HttpRequestBuilder
import io.ktor.client.request.accept
import io.ktor.client.request.bearerAuth
import io.ktor.client.request.header
import io.ktor.client.request.request
import io.ktor.client.request.setBody
import io.ktor.client.statement.HttpResponse
import io.ktor.http.ContentType
import io.ktor.http.HttpHeaders
import io.ktor.http.HttpMethod
import io.ktor.http.contentType
import io.ktor.serialization.kotlinx.json.json
import io.ktor.websocket.Frame
import io.ktor.websocket.readText
import kotlinx.coroutines.flow.Flow
import kotlinx.coroutines.flow.flow
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** Errors surfaced to view models. [Unauthorized] drives a return to the login screen. */
sealed class FridayException : Exception() {
    data object NotConfigured : FridayException()
    data object Unauthorized : FridayException()
    data class Http(val code: Int) : FridayException()
    data object Transport : FridayException()
}

/**
 * The Friday cloud surface the threads feature needs, backed by Ktor. Reads the
 * active base URL and bearer token from [session] for every call.
 */
class FridayClient(private val session: Session) {

    private val json = Json {
        ignoreUnknownKeys = true
        isLenient = true
        explicitNulls = false
    }

    private val http = HttpClient {
        expectSuccess = false
        install(ContentNegotiation) { json(json) }
        install(WebSockets)
    }

    // MARK: Auth

    suspend fun login(baseUrl: String, username: String, password: String) =
        authenticate("/api/login", baseUrl, username, password)

    suspend fun register(baseUrl: String, username: String, password: String) =
        authenticate("/api/register", baseUrl, username, password)

    suspend fun signOut() {
        runCatching { authed(HttpMethod.Post, "/api/logout") {} }
        session.signOut()
    }

    private suspend fun authenticate(path: String, baseUrl: String, username: String, password: String) {
        val response = try {
            http.request(join(baseUrl, path)) {
                method = HttpMethod.Post
                accept(ContentType.Application.Json)
                contentType(ContentType.Application.Json)
                setBody(AuthBody(username, password))
            }
        } catch (e: FridayException) {
            throw e
        } catch (_: Throwable) {
            throw FridayException.Transport
        }
        validate(response)
        session.signIn(baseUrl, response.body<AuthResponse>().token)
    }

    // MARK: Threads

    suspend fun listThreads(): List<ThreadSummary> = authed(HttpMethod.Get, "/api/threads") {}.body()

    suspend fun listProjects(): List<Project> = authed(HttpMethod.Get, "/api/projects") {}.body()

    suspend fun listMessages(threadId: String): List<Message> =
        authed(HttpMethod.Get, "/api/threads/$threadId/messages") {}.body()

    suspend fun createThread(content: String, projectId: String?, filePaths: List<String>): SendResult =
        authed(HttpMethod.Post, "/api/threads") {
            contentType(ContentType.Application.Json)
            setBody(CreateThreadBody(content, projectId, filePaths))
        }.body()

    suspend fun sendMessage(threadId: String, content: String, filePaths: List<String>): SendResult =
        authed(HttpMethod.Post, "/api/threads/$threadId/messages") {
            contentType(ContentType.Application.Json)
            setBody(SendMessageBody(content, filePaths))
        }.body()

    suspend fun cancelRun(threadId: String) {
        authed(HttpMethod.Post, "/api/threads/$threadId/cancel") {}
    }

    suspend fun resolveApproval(threadId: String, approvalId: String, approved: Boolean) {
        authed(HttpMethod.Post, "/api/threads/$threadId/approvals/$approvalId") {
            contentType(ContentType.Application.Json)
            setBody(ApprovalBody(approved))
        }
    }

    suspend fun answerQuiz(threadId: String, quizId: String, answers: List<String>) {
        authed(HttpMethod.Post, "/api/threads/$threadId/quizzes/$quizId") {
            contentType(ContentType.Application.Json)
            setBody(QuizAnswerBody(answers))
        }
    }

    /**
     * Streams thread events over a WebSocket. Completes when the socket closes;
     * the caller is responsible for reconnecting. Unparseable frames are dropped.
     */
    fun events(threadId: String): Flow<ThreadEvent> = flow {
        val base = session.baseUrl ?: return@flow
        val token = session.token ?: return@flow
        http.webSocket(
            urlString = webSocketUrl(base, threadId),
            request = { header(HttpHeaders.Authorization, "Bearer $token") },
        ) {
            for (frame in incoming) {
                val text = (frame as? Frame.Text)?.readText() ?: continue
                val event = runCatching { json.decodeFromString<ThreadEvent>(text) }.getOrNull() ?: continue
                emit(event)
            }
        }
    }

    // MARK: Plumbing

    private suspend fun authed(
        method: HttpMethod,
        path: String,
        block: HttpRequestBuilder.() -> Unit,
    ): HttpResponse {
        val base = session.baseUrl ?: throw FridayException.NotConfigured
        val response = try {
            http.request(join(base, path)) {
                this.method = method
                accept(ContentType.Application.Json)
                session.token?.let { bearerAuth(it) }
                block()
            }
        } catch (e: FridayException) {
            throw e
        } catch (_: Throwable) {
            throw FridayException.Transport
        }
        validate(response)
        return response
    }

    private fun validate(response: HttpResponse) {
        when (val code = response.status.value) {
            in 200..299 -> Unit
            401 -> throw FridayException.Unauthorized
            else -> throw FridayException.Http(code)
        }
    }

    private fun join(baseUrl: String, path: String): String = baseUrl.trimEnd('/') + path

    private fun webSocketUrl(baseUrl: String, threadId: String): String {
        val trimmed = baseUrl.trimEnd('/')
        val ws = when {
            trimmed.startsWith("https://") -> "wss://" + trimmed.removePrefix("https://")
            trimmed.startsWith("http://") -> "ws://" + trimmed.removePrefix("http://")
            else -> trimmed
        }
        return "$ws/api/threads/$threadId/events"
    }
}

@Serializable
private data class AuthBody(val username: String, val password: String)

@Serializable
private data class AuthResponse(val token: String)

@Serializable
private data class CreateThreadBody(
    val content: String,
    @SerialName("project_id") val projectId: String?,
    @SerialName("file_paths") val filePaths: List<String>,
)

@Serializable
private data class SendMessageBody(
    val content: String,
    @SerialName("file_paths") val filePaths: List<String>,
)

@Serializable
private data class ApprovalBody(val approved: Boolean)

@Serializable
private data class QuizAnswerBody(val answers: List<String>)
