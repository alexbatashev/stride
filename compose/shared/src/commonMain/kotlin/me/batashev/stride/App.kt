package me.batashev.stride

import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import me.batashev.stride.ui.MainScreen
import me.batashev.stride.ui.auth.AuthScreen
import me.batashev.stride.ui.theme.StrideTheme

@Composable
fun App(container: AppContainer = remember { AppContainer() }) {
    StrideTheme {
        val auth by container.session.state.collectAsState()
        Surface(modifier = Modifier.fillMaxSize(), color = MaterialTheme.colorScheme.background) {
            if (auth.isAuthenticated) {
                MainScreen(container)
            } else {
                AuthScreen(container)
            }
        }
    }
}
