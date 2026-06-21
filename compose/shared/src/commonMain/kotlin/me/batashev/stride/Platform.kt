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
