package me.batashev.stride

import android.annotation.SuppressLint
import android.content.Context
import android.os.Build
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.platform.LocalContext

/** Holds the application context so [createSettings] can reach SharedPreferences. */
object AndroidApp {
    @SuppressLint("StaticFieldLeak")
    lateinit var context: Context

    fun init(context: Context) {
        this.context = context.applicationContext
    }
}

actual fun createSettings(): Settings = object : Settings {
    private val prefs = AndroidApp.context.getSharedPreferences("friday.session", Context.MODE_PRIVATE)

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
