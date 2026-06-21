package me.batashev.stride.data

import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import me.batashev.stride.Settings

/**
 * Holds the cloud server base URL and bearer token for the signed-in user and
 * mirrors them to persistent [Settings]. The base URL is kept after sign-out so
 * the login screen can prefill it.
 */
class Session(private val settings: Settings) {

    data class Auth(val baseUrl: String?, val token: String?) {
        val isAuthenticated: Boolean
            get() = !baseUrl.isNullOrBlank() && !token.isNullOrBlank()
    }

    private val _state = MutableStateFlow(read())
    val state: StateFlow<Auth> = _state.asStateFlow()

    val baseUrl: String? get() = _state.value.baseUrl
    val token: String? get() = _state.value.token
    val isAuthenticated: Boolean get() = _state.value.isAuthenticated

    fun signIn(baseUrl: String, token: String) {
        settings.putString(KEY_BASE, baseUrl)
        settings.putString(KEY_TOKEN, token)
        _state.value = Auth(baseUrl, token)
    }

    fun signOut() {
        settings.remove(KEY_TOKEN)
        _state.value = _state.value.copy(token = null)
    }

    private fun read() = Auth(settings.getString(KEY_BASE), settings.getString(KEY_TOKEN))

    private companion object {
        const val KEY_BASE = "friday.baseURL"
        const val KEY_TOKEN = "friday.token"
    }
}
