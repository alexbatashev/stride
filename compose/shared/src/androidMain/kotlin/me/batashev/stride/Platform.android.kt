package me.batashev.stride

import android.annotation.SuppressLint
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.core.content.FileProvider
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

/** Holds the application context so [createSettings] can reach SharedPreferences. */
object AndroidApp {
    @SuppressLint("StaticFieldLeak")
    lateinit var context: Context

    fun init(context: Context) {
        this.context = context.applicationContext
    }
}

actual fun createSettings(): Settings = object : Settings {
    private val prefs = AndroidApp.context.getSharedPreferences("stride.session", Context.MODE_PRIVATE)

    override fun getString(key: String): String? = prefs.getString(key, null)

    override fun putString(key: String, value: String) {
        prefs.edit().putString(key, value).apply()
    }

    override fun remove(key: String) {
        prefs.edit().remove(key).apply()
    }
}

@Composable
actual fun dynamicColorSchemeOrNull(darkTheme: Boolean): ColorScheme? {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S) return null
    val context = LocalContext.current
    return if (darkTheme) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
}

@Composable
actual fun SystemBackHandler(enabled: Boolean, onBack: () -> Unit) {
    androidx.activity.compose.BackHandler(enabled = enabled, onBack = onBack)
}

@Composable
actual fun rememberFilePicker(onResult: (PickedFile?) -> Unit): () -> Unit {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val launcher = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) {
            onResult(null)
        } else {
            scope.launch(Dispatchers.IO) { onResult(readPickedFile(context, uri)) }
        }
    }
    return { launcher.launch(arrayOf("*/*")) }
}

private fun readPickedFile(context: Context, uri: Uri): PickedFile? = runCatching {
    val resolver = context.contentResolver
    val name = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { cursor ->
        if (cursor.moveToFirst()) cursor.getString(0) else null
    } ?: uri.lastPathSegment ?: "file"
    val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: return null
    PickedFile(name, resolver.getType(uri), bytes)
}.getOrNull()

actual suspend fun openFile(name: String, mimeType: String?, bytes: ByteArray) = withContext(Dispatchers.IO) {
    val context = AndroidApp.context
    val dir = File(context.cacheDir, "shared").apply { mkdirs() }
    val file = File(dir, name.substringAfterLast('/').ifEmpty { "file" })
    file.writeBytes(bytes)
    val uri = FileProvider.getUriForFile(context, "${context.packageName}.fileprovider", file)
    val view = Intent(Intent.ACTION_VIEW).apply {
        setDataAndType(uri, mimeType ?: "*/*")
        addFlags(Intent.FLAG_GRANT_READ_URI_PERMISSION)
    }
    val chooser = Intent.createChooser(view, null).apply { addFlags(Intent.FLAG_ACTIVITY_NEW_TASK) }
    context.startActivity(chooser)
}
