package me.batashev.stride.ui

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.Chat
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.NavigationRail
import androidx.compose.material3.NavigationRailItem
import androidx.compose.material3.Text
import androidx.compose.material3.adaptive.ExperimentalMaterial3AdaptiveApi
import androidx.compose.material3.adaptive.layout.AnimatedPane
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffold
import androidx.compose.material3.adaptive.layout.ListDetailPaneScaffoldRole
import androidx.compose.material3.adaptive.navigation.rememberListDetailPaneScaffoldNavigator
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.saveable.rememberSaveableStateHolder
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import androidx.lifecycle.viewmodel.initializer
import androidx.lifecycle.viewmodel.viewModelFactory
import kotlinx.coroutines.launch
import me.batashev.stride.AppContainer
import me.batashev.stride.SystemBackHandler
import me.batashev.stride.ui.chat.ChatEmptyState
import me.batashev.stride.ui.chat.ChatPane
import me.batashev.stride.ui.chat.ChatViewModel
import me.batashev.stride.ui.files.FilesPane
import me.batashev.stride.ui.files.FilesViewModel
import me.batashev.stride.ui.threads.ThreadListPane
import me.batashev.stride.ui.threads.ThreadsViewModel

private const val NEW_PREFIX = "new-"

private enum class Destination(val label: String, val icon: ImageVector) {
    Chats("Chats", Icons.AutoMirrored.Filled.Chat),
    Files("Files", Icons.Filled.Folder),
}

private const val EXPANDED_WIDTH_DP = 600

@Composable
fun MainScreen(container: AppContainer) {
    var selected by rememberSaveable { mutableStateOf(0) }
    val stateHolder = rememberSaveableStateHolder()

    AdaptiveNavigation(selected = selected, onSelect = { selected = it }) {
        stateHolder.SaveableStateProvider(selected) {
            when (Destination.entries[selected]) {
                Destination.Chats -> ChatsScreen(container)
                Destination.Files -> FilesScreen(container)
            }
        }
    }
}

@Composable
private fun AdaptiveNavigation(selected: Int, onSelect: (Int) -> Unit, content: @Composable () -> Unit) {
    BoxWithConstraints(modifier = Modifier.fillMaxSize()) {
        if (maxWidth >= EXPANDED_WIDTH_DP.dp) {
            Row(modifier = Modifier.fillMaxSize()) {
                NavigationRail {
                    Destination.entries.forEachIndexed { index, destination ->
                        NavigationRailItem(
                            selected = index == selected,
                            onClick = { onSelect(index) },
                            icon = { Icon(destination.icon, contentDescription = destination.label) },
                            label = { Text(destination.label) },
                        )
                    }
                }
                Box(modifier = Modifier.weight(1f).fillMaxHeight()) { content() }
            }
        } else {
            Column(modifier = Modifier.fillMaxSize()) {
                Box(modifier = Modifier.weight(1f).fillMaxWidth()) { content() }
                NavigationBar {
                    Destination.entries.forEachIndexed { index, destination ->
                        NavigationBarItem(
                            selected = index == selected,
                            onClick = { onSelect(index) },
                            icon = { Icon(destination.icon, contentDescription = destination.label) },
                            label = { Text(destination.label) },
                        )
                    }
                }
            }
        }
    }
}

@OptIn(ExperimentalMaterial3AdaptiveApi::class)
@Composable
private fun ChatsScreen(container: AppContainer) {
    val scope = rememberCoroutineScope()
    val onUnauthorized: () -> Unit = { container.session.signOut() }

    val threadsVm: ThreadsViewModel = viewModel(
        factory = viewModelFactory { initializer { ThreadsViewModel(container.client, onUnauthorized) } },
    )
    val threadsState by threadsVm.state.collectAsStateWithLifecycle()

    val navigator = rememberListDetailPaneScaffoldNavigator<String>()
    var newCounter by remember { mutableIntStateOf(0) }

    val selectedKey = navigator.currentDestination?.contentKey
    val showBack = navigator.canNavigateBack()

    SystemBackHandler(enabled = showBack) { scope.launch { navigator.navigateBack() } }

    fun openThread(id: String) {
        scope.launch { navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, id) }
    }
    fun newThread() {
        newCounter += 1
        scope.launch { navigator.navigateTo(ListDetailPaneScaffoldRole.Detail, "$NEW_PREFIX$newCounter") }
    }

    ListDetailPaneScaffold(
        directive = navigator.scaffoldDirective,
        value = navigator.scaffoldValue,
        listPane = {
            AnimatedPane {
                ThreadListPane(
                    state = threadsState,
                    selectedThreadId = selectedKey?.takeUnless { it.startsWith(NEW_PREFIX) },
                    onQueryChange = threadsVm::setQuery,
                    onRefresh = threadsVm::refresh,
                    onOpen = { openThread(it) },
                    onNew = { newThread() },
                    onSignOut = { scope.launch { container.client.signOut() } },
                )
            }
        },
        detailPane = {
            AnimatedPane {
                ChatDetail(
                    contentKey = selectedKey,
                    container = container,
                    threadsState = threadsState,
                    showBack = showBack,
                    onThreadsChanged = threadsVm::refresh,
                    onUnauthorized = onUnauthorized,
                    onBack = { scope.launch { navigator.navigateBack() } },
                    onNew = { newThread() },
                )
            }
        },
    )
}

@Composable
private fun FilesScreen(container: AppContainer) {
    val onUnauthorized: () -> Unit = { container.session.signOut() }
    val vm: FilesViewModel = viewModel(
        factory = viewModelFactory { initializer { FilesViewModel(container.client, onUnauthorized) } },
    )
    FilesPane(vm = vm)
}

@Composable
private fun ChatDetail(
    contentKey: String?,
    container: AppContainer,
    threadsState: ThreadsViewModel.UiState,
    showBack: Boolean,
    onThreadsChanged: () -> Unit,
    onUnauthorized: () -> Unit,
    onBack: () -> Unit,
    onNew: () -> Unit,
) {
    if (contentKey == null) {
        ChatEmptyState(onNew = onNew)
        return
    }

    val threadId = contentKey.takeUnless { it.startsWith(NEW_PREFIX) }
    val title = when {
        threadId == null -> "New conversation"
        else -> threadsState.threads.firstOrNull { it.id == threadId }?.title ?: "Conversation"
    }

    val vm: ChatViewModel = viewModel(
        key = contentKey,
        factory = viewModelFactory {
            initializer { ChatViewModel(container.client, threadId, onThreadsChanged, onUnauthorized) }
        },
    )

    ChatPane(title = title, vm = vm, showBack = showBack, onBack = onBack)
}
