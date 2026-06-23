package me.batashev.stride.ui.auth

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.AutoAwesome
import androidx.compose.material3.Card
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.FilterChip
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Button
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import me.batashev.stride.AppContainer

@Composable
fun AuthScreen(container: AppContainer) {
    val vm: AuthViewModel = viewModel(
        factory = viewModelFactory { initializer { AuthViewModel(container.client, container.session) } },
    )
    val state by vm.state.collectAsStateWithLifecycle()

    Box(modifier = Modifier.fillMaxSize().padding(24.dp), contentAlignment = Alignment.Center) {
        Card(modifier = Modifier.widthIn(max = 420.dp).fillMaxWidth()) {
            Column(
                modifier = Modifier.padding(28.dp),
                verticalArrangement = Arrangement.spacedBy(18.dp),
            ) {
                Column(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    Icon(
                        imageVector = Icons.Filled.AutoAwesome,
                        contentDescription = null,
                        modifier = Modifier.size(40.dp),
                        tint = MaterialTheme.colorScheme.primary,
                    )
                    Text("S.T.R.I.D.E.", style = MaterialTheme.typography.headlineSmall)
                    Text(
                        text = if (state.mode == AuthViewModel.Mode.Login) "Sign in to your cloud agent" else "Create your account",
                        style = MaterialTheme.typography.bodyMedium,
                        color = MaterialTheme.colorScheme.onSurfaceVariant,
                    )
                }

                Row(horizontalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.fillMaxWidth()) {
                    FilterChip(
                        selected = state.mode == AuthViewModel.Mode.Login,
                        onClick = { vm.setMode(AuthViewModel.Mode.Login) },
                        label = { Text("Sign in") },
                        modifier = Modifier.weight(1f),
                    )
                    FilterChip(
                        selected = state.mode == AuthViewModel.Mode.Register,
                        onClick = { vm.setMode(AuthViewModel.Mode.Register) },
                        label = { Text("Register") },
                        modifier = Modifier.weight(1f),
                    )
                }

                OutlinedTextField(
                    value = state.serverUrl,
                    onValueChange = vm::setServerUrl,
                    label = { Text("Server URL") },
                    placeholder = { Text("https://stride.example.com") },
                    singleLine = true,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = state.username,
                    onValueChange = vm::setUsername,
                    label = { Text("Username") },
                    singleLine = true,
                    modifier = Modifier.fillMaxWidth(),
                )
                OutlinedTextField(
                    value = state.password,
                    onValueChange = vm::setPassword,
                    label = { Text("Password") },
                    singleLine = true,
                    visualTransformation = PasswordVisualTransformation(),
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password),
                    modifier = Modifier.fillMaxWidth(),
                )

                state.error?.let {
                    Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodySmall)
                }

                Button(
                    onClick = vm::submit,
                    enabled = state.canSubmit,
                    modifier = Modifier.fillMaxWidth(),
                ) {
                    if (state.submitting) {
                        CircularProgressIndicator(
                            modifier = Modifier.size(20.dp),
                            strokeWidth = 2.dp,
                            color = MaterialTheme.colorScheme.onPrimary,
                        )
                    } else {
                        Text(if (state.mode == AuthViewModel.Mode.Login) "Sign in" else "Create account")
                    }
                }
            }
        }
    }
}
