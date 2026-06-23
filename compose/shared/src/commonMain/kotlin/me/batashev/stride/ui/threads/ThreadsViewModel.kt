package me.batashev.stride.ui.threads

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import me.batashev.stride.data.FridayClient
import me.batashev.stride.data.FridayException
import me.batashev.stride.data.Project
import me.batashev.stride.data.ThreadSummary

class ThreadsViewModel(
    private val client: FridayClient,
    private val onUnauthorized: () -> Unit,
) : ViewModel() {

    data class UiState(
        val isLoading: Boolean = false,
        val threads: List<ThreadSummary> = emptyList(),
        val projects: List<Project> = emptyList(),
        val query: String = "",
        val error: String? = null,
    ) {
        val visibleThreads: List<ThreadSummary>
            get() {
                val q = query.trim().lowercase()
                if (q.isEmpty()) return threads
                return threads.filter { it.title.lowercase().contains(q) }
            }

        fun projectTitle(thread: ThreadSummary): String {
            val id = thread.projectId ?: return "Friday"
            return projects.firstOrNull { it.id == id }?.title ?: "Friday"
        }
    }

    private val _state = MutableStateFlow(UiState(isLoading = true))
    val state: StateFlow<UiState> = _state.asStateFlow()

    init {
        refresh()
    }

    fun setQuery(value: String) = _state.update { it.copy(query = value) }

    fun refresh() {
        _state.update { it.copy(isLoading = true, error = null) }
        viewModelScope.launch {
            try {
                val (threads, projects) = coroutineScope {
                    val threads = async { client.listThreads() }
                    val projects = async { client.listProjects() }
                    threads.await() to projects.await()
                }
                _state.update { it.copy(isLoading = false, error = null, threads = threads, projects = projects) }
            } catch (e: FridayException.Unauthorized) {
                onUnauthorized()
            } catch (e: Throwable) {
                _state.update { it.copy(isLoading = false, error = "Couldn't load your conversations.") }
            }
        }
    }
}
