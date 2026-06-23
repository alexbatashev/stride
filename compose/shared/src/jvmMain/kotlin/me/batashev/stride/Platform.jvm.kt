package me.batashev.stride

import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.awt.Desktop
import java.awt.FileDialog
import java.awt.Frame
import java.io.File
import java.nio.file.Files
import java.util.prefs.Preferences

actual fun createSettings(): Settings = object : Settings {
    private val prefs = Preferences.userRoot().node("me/batashev/stride")

    override fun getString(key: String): String? = prefs.get(key, null)

    override fun putString(key: String, value: String) {
        prefs.put(key, value)
        prefs.flush()
    }

    override fun remove(key: String) {
        prefs.remove(key)
        prefs.flush()
    }
}

@Composable
actual fun dynamicColorSchemeOrNull(darkTheme: Boolean): ColorScheme? = null

@Composable
actual fun SystemBackHandler(enabled: Boolean, onBack: () -> Unit) {
    // Desktop has no system back gesture; the in-app back button drives navigation.
}

@Composable
actual fun rememberFilePicker(onResult: (PickedFile?) -> Unit): () -> Unit {
    val scope = rememberCoroutineScope()
    return {
        scope.launch(Dispatchers.IO) {
            val dialog = FileDialog(null as Frame?, "Choose a file", FileDialog.LOAD)
            dialog.isVisible = true
            val directory = dialog.directory
            val name = dialog.file
            val picked = if (directory != null && name != null) {
                runCatching {
                    val file = File(directory, name)
                    PickedFile(file.name, Files.probeContentType(file.toPath()), file.readBytes())
                }.getOrNull()
            } else {
                null
            }
            onResult(picked)
        }
    }
}

actual suspend fun openFile(name: String, mimeType: String?, bytes: ByteArray) = withContext(Dispatchers.IO) {
    val dir = Files.createTempDirectory("stride").toFile()
    val file = File(dir, name.substringAfterLast('/').ifEmpty { "file" })
    file.writeBytes(bytes)
    if (Desktop.isDesktopSupported() && Desktop.getDesktop().isSupported(Desktop.Action.OPEN)) {
        Desktop.getDesktop().open(file)
    }
}
