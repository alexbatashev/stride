package me.batashev.stride

import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable

/** Minimal cross-platform key/value persistence for the signed-in session. */
interface Settings {
    fun getString(key: String): String?
    fun putString(key: String, value: String)
    fun remove(key: String)
}

expect fun createSettings(): Settings

/** Material You dynamic color where the platform supports it, otherwise null. */
@Composable
expect fun dynamicColorSchemeOrNull(darkTheme: Boolean): ColorScheme?

/** Routes the platform back gesture/button to [onBack] when [enabled]. No-op on desktop. */
@Composable
expect fun SystemBackHandler(enabled: Boolean, onBack: () -> Unit)

/** A file the user picked for upload: its display name, MIME type and raw bytes. */
class PickedFile(val name: String, val mimeType: String?, val bytes: ByteArray)

/**
 * Remembers a platform file picker and returns a function that launches it. The
 * result is delivered to [onResult]; a null value means the user cancelled.
 */
@Composable
expect fun rememberFilePicker(onResult: (PickedFile?) -> Unit): () -> Unit

/** Writes [bytes] to a temporary file and opens it with the platform's default viewer. */
expect suspend fun openFile(name: String, mimeType: String?, bytes: ByteArray)
