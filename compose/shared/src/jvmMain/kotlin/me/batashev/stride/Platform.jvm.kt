package me.batashev.stride

import androidx.compose.material3.ColorScheme
import androidx.compose.runtime.Composable
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
