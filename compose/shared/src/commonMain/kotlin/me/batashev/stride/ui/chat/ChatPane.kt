package me.batashev.stride.ui.chat

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle

private val ReadingWidth = 720.dp

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun ChatPane(
    title: String,
    vm: ChatViewModel,
    showBack: Boolean,
    onBack: () -> Unit,
) {
    val state by vm.state.collectAsStateWithLifecycle()
    var draft by rememberSaveable(vm) { mutableStateOf("") }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    if (showBack) {
                        IconButton(onClick = onBack) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Back")
                        }
                    }
                },
                actions = {
                    if (state.running) {
                        IconButton(onClick = vm::cancel) {
                            Icon(Icons.Filled.Stop, contentDescription = "Stop")
                        }
                    }
                },
            )
        },
        bottomBar = {
            BottomArea(
                state = state,
                draft = draft,
                onDraftChange = { draft = it },
                onSend = {
                    vm.send(draft)
                    draft = ""
                },
                onStop = vm::cancel,
                onApprove = { vm.resolveApproval(true) },
                onDeny = { vm.resolveApproval(false) },
                onQuiz = vm::answerQuiz,
                onDismissError = vm::dismissError,
            )
        },
    ) { padding ->
        MessageList(
            state = state,
            modifier = Modifier.fillMaxSize().padding(padding),
        )
    }
}

@Composable
private fun MessageList(state: ChatViewModel.UiState, modifier: Modifier) {
    val listState = rememberLazyListState()
    val scrollSignal = state.messages.size + (state.streaming?.content?.length ?: 0) +
        (if (state.activeTool != null) 1 else 0) + (if (state.running) 1 else 0)

    LaunchedEffect(scrollSignal) {
        val count = listState.layoutInfo.totalItemsCount
        if (count > 0) listState.animateScrollToItem(count - 1)
    }

    Box(modifier = modifier, contentAlignment = Alignment.TopCenter) {
        if (state.messages.isEmpty() && state.streaming == null && !state.loadingHistory) {
            NewConversationHint()
        }
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize().widthIn(max = ReadingWidth),
            contentPadding = androidx.compose.foundation.layout.PaddingValues(16.dp),
            verticalArrangement = Arrangement.spacedBy(18.dp),
        ) {
            if (state.loadingHistory) {
                item { CircularProgressIndicator(modifier = Modifier.padding(top = 24.dp)) }
            }
            items(state.messages, key = { it.id }) { message ->
                MessageRow(message)
            }
            state.streaming?.let { streaming ->
                item("streaming") { StreamingRow(streaming) }
            }
            val tool = state.activeTool
            when {
                tool != null -> item("tool") { ToolActivityRow(tool) }
                state.running && state.streaming == null -> item("typing") { TypingIndicator() }
            }
        }
    }
}

@Composable
private fun BottomArea(
    state: ChatViewModel.UiState,
    draft: String,
    onDraftChange: (String) -> Unit,
    onSend: () -> Unit,
    onStop: () -> Unit,
    onApprove: () -> Unit,
    onDeny: () -> Unit,
    onQuiz: (String) -> Unit,
    onDismissError: () -> Unit,
) {
    Box(modifier = Modifier.fillMaxWidth().imePadding(), contentAlignment = Alignment.BottomCenter) {
        Column(
            modifier = Modifier.fillMaxWidth().widthIn(max = ReadingWidth).padding(horizontal = 16.dp, vertical = 10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            state.error?.let { ErrorBanner(it, onDismiss = onDismissError) }

            val approval = state.approval
            val quiz = state.quiz
            val quizQuestion = quiz?.current
            when {
                approval != null -> ApprovalCard(approval.message, onApprove = onApprove, onDeny = onDeny)
                quiz != null && quizQuestion != null -> QuizCard(
                    question = quizQuestion,
                    progress = quiz.progress,
                    onSelect = onQuiz,
                )
                else -> Composer(
                    draft = draft,
                    onDraftChange = onDraftChange,
                    running = state.running,
                    canSend = state.canSend(draft),
                    onSend = onSend,
                    onStop = onStop,
                )
            }
        }
    }
}

@Composable
private fun NewConversationHint() {
    Column(
        modifier = Modifier.fillMaxSize().padding(32.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text("What are we working on?", style = MaterialTheme.typography.titleLarge)
        Text(
            text = "Ask S.T.R.I.D.E. anything to get started.",
            style = MaterialTheme.typography.bodyMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
    }
}
