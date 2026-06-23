package me.batashev.stride.ui.auth

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import me.batashev.stride.data.StrideClient
import me.batashev.stride.data.StrideException
import me.batashev.stride.data.Session

class AuthViewModel(
    private val client: StrideClient,
    session: Session,
) : ViewModel() {

    enum class Mode { Login, Register }

    data class UiState(
        val mode: Mode = Mode.Login,
        val serverUrl: String = "",
        val username: String = "",
        val password: String = "",
        val submitting: Boolean = false,
        val error: String? = null,
    ) {
        val canSubmit: Boolean
            get() = serverUrl.isNotBlank() && username.isNotBlank() && password.isNotEmpty() && !submitting
    }

    private val _state = MutableStateFlow(UiState(serverUrl = session.baseUrl ?: ""))
    val state: StateFlow<UiState> = _state.asStateFlow()

    fun setMode(mode: Mode) = _state.update { it.copy(mode = mode, error = null) }
    fun setServerUrl(value: String) = _state.update { it.copy(serverUrl = value) }
    fun setUsername(value: String) = _state.update { it.copy(username = value) }
    fun setPassword(value: String) = _state.update { it.copy(password = value) }

    fun submit() {
        val current = _state.value
        if (!current.canSubmit) return
        val url = normalizedUrl(current.serverUrl)
        if (url == null) {
            _state.update { it.copy(error = "Enter a valid server URL.") }
            return
        }
        val username = current.username.trim()
        val password = current.password
        val mode = current.mode

        _state.update { it.copy(submitting = true, error = null) }
        viewModelScope.launch {
            try {
                if (mode == Mode.Login) client.login(url, username, password)
                else client.register(url, username, password)
                // Success flips Session.isAuthenticated; the root recomposes to the main UI.
            } catch (e: Throwable) {
                _state.update { it.copy(submitting = false, error = message(e)) }
            }
        }
    }

    private fun message(error: Throwable): String = when (error) {
        is StrideException.Unauthorized -> "Wrong username or password."
        is StrideException.Http -> "Server error (${error.code})."
        is StrideException.NotConfigured -> "Enter a valid server URL."
        else -> "Couldn't reach the server."
    }

    private fun normalizedUrl(raw: String): String? {
        val trimmed = raw.trim()
        if (trimmed.isEmpty()) return null
        val withScheme = if (trimmed.startsWith("http://") || trimmed.startsWith("https://")) {
            trimmed
        } else {
            "https://$trimmed"
        }
        return withScheme.trimEnd('/').takeIf { it.length > "https://".length }
    }
}
