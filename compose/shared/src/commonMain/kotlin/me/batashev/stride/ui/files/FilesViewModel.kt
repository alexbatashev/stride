package me.batashev.stride.ui.files

import androidx.lifecycle.ViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import me.batashev.stride.PickedFile
import me.batashev.stride.data.FileEntry
import me.batashev.stride.data.StrideClient
import me.batashev.stride.data.StrideException
import me.batashev.stride.openFile

/**
 * Browses the user's global files. Navigation, listing and mutations all funnel
 * through [path]; every successful mutation reloads the current directory. Ports
 * the reducer logic from the Apple app's `FilesFeature` (global scope).
 */
class FilesViewModel(
    private val client: StrideClient,
    private val onUnauthorized: () -> Unit,
) : ViewModel() {

    sealed interface Dialog {
        data object NewFolder : Dialog
        data class Rename(val entry: FileEntry) : Dialog
        data class Delete(val entry: FileEntry) : Dialog
    }

    data class UiState(
        val path: String = "",
        val entries: List<FileEntry> = emptyList(),
        val isLoading: Boolean = false,
        val busy: Boolean = false,
        val error: String? = null,
        val dialog: Dialog? = null,
    ) {
        val canGoUp: Boolean get() = path.isNotEmpty()
        val title: String get() = if (path.isEmpty()) "Files" else path.substringAfterLast('/')
    }

    private val _state = MutableStateFlow(UiState(isLoading = true))
    val state: StateFlow<UiState> = _state.asStateFlow()

    init {
        load()
    }

    fun refresh() = load()

    fun open(entry: FileEntry) {
        if (entry.isDirectory) {
            _state.update { it.copy(path = entry.path) }
            load()
        } else {
            downloadAndOpen(entry)
        }
    }

    fun goUp() {
        val current = _state.value.path
        if (current.isEmpty()) return
        _state.update { it.copy(path = current.substringBeforeLast('/', "")) }
        load()
    }

    fun showNewFolder() = _state.update { it.copy(dialog = Dialog.NewFolder) }

    fun showRename(entry: FileEntry) = _state.update { it.copy(dialog = Dialog.Rename(entry)) }

    fun showDelete(entry: FileEntry) = _state.update { it.copy(dialog = Dialog.Delete(entry)) }

    fun dismissDialog() = _state.update { it.copy(dialog = null) }

    fun dismissError() = _state.update { it.copy(error = null) }

    fun createFolder(name: String) {
        val trimmed = name.trim()
        if (trimmed.isEmpty()) {
            dismissDialog()
            return
        }
        val target = joinPath(_state.value.path, trimmed)
        mutate { client.createDirectory(target) }
    }

    fun rename(entry: FileEntry, newName: String) {
        val trimmed = newName.trim()
        if (trimmed.isEmpty() || trimmed == entry.name) {
            dismissDialog()
            return
        }
        mutate { client.renameFile(entry.path, trimmed) }
    }

    fun delete(entry: FileEntry) = mutate { client.deleteFile(entry.path) }

    fun upload(file: PickedFile) = mutate { client.uploadFile(_state.value.path, file) }

    private fun downloadAndOpen(entry: FileEntry) {
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            try {
                val bytes = client.downloadFile(entry.path)
                openFile(entry.name, entry.mimeType, bytes)
                _state.update { it.copy(busy = false) }
            } catch (e: StrideException.Unauthorized) {
                onUnauthorized()
            } catch (e: Throwable) {
                _state.update { it.copy(busy = false, error = "Couldn't open ${entry.name}.") }
            }
        }
    }

    private fun mutate(action: suspend () -> Unit) {
        _state.update { it.copy(busy = true, error = null, dialog = null) }
        viewModelScope.launch {
            try {
                action()
                fetch()
            } catch (e: StrideException.Unauthorized) {
                onUnauthorized()
            } catch (e: Throwable) {
                _state.update { it.copy(busy = false, error = "Something went wrong. Try again.") }
            }
        }
    }

    private fun load() {
        _state.update { it.copy(isLoading = true, error = null) }
        viewModelScope.launch { fetch() }
    }

    private suspend fun fetch() {
        try {
            val listing = client.listFiles(_state.value.path)
            _state.update {
                it.copy(
                    isLoading = false,
                    busy = false,
                    path = listing.path,
                    entries = sortEntries(listing.entries),
                )
            }
        } catch (e: StrideException.Unauthorized) {
            onUnauthorized()
        } catch (e: Throwable) {
            _state.update { it.copy(isLoading = false, busy = false, error = "Couldn't load your files.") }
        }
    }

    private fun sortEntries(entries: List<FileEntry>): List<FileEntry> =
        entries.sortedWith(compareByDescending<FileEntry> { it.isDirectory }.thenBy { it.name.lowercase() })

    private fun joinPath(parent: String, name: String): String = if (parent.isEmpty()) name else "$parent/$name"
}
