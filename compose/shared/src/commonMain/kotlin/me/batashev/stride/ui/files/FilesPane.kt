package me.batashev.stride.ui.files

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.InsertDriveFile
import androidx.compose.material.icons.filled.Close
import androidx.compose.material.icons.filled.CreateNewFolder
import androidx.compose.material.icons.filled.Delete
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.ErrorOutline
import androidx.compose.material.icons.filled.Folder
import androidx.compose.material.icons.filled.FolderOpen
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material.icons.filled.Upload
import androidx.compose.material3.AlertDialog
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import me.batashev.stride.data.FileEntry
import me.batashev.stride.rememberFilePicker
import me.batashev.stride.ui.files.FilesViewModel.Dialog
import me.batashev.stride.util.formatDate
import me.batashev.stride.util.formatSize

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun FilesPane(vm: FilesViewModel) {
    val state by vm.state.collectAsStateWithLifecycle()
    val launchPicker = rememberFilePicker { picked -> picked?.let(vm::upload) }

    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(state.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                navigationIcon = {
                    if (state.canGoUp) {
                        IconButton(onClick = vm::goUp) {
                            Icon(Icons.AutoMirrored.Filled.ArrowBack, contentDescription = "Up")
                        }
                    }
                },
                actions = {
                    IconButton(onClick = vm::showNewFolder) {
                        Icon(Icons.Filled.CreateNewFolder, contentDescription = "New folder")
                    }
                    IconButton(onClick = launchPicker) {
                        Icon(Icons.Filled.Upload, contentDescription = "Upload")
                    }
                },
            )
        },
    ) { padding ->
        Column(modifier = Modifier.fillMaxSize().padding(padding)) {
            if (state.busy) LinearProgressIndicator(modifier = Modifier.fillMaxWidth())
            state.error?.let { ErrorBanner(message = it, onDismiss = vm::dismissError) }

            PullToRefreshBox(
                isRefreshing = state.isLoading,
                onRefresh = vm::refresh,
                modifier = Modifier.fillMaxSize(),
            ) {
                when {
                    state.isLoading && state.entries.isEmpty() -> CenteredProgress()
                    state.entries.isEmpty() -> EmptyFiles(onUpload = launchPicker)
                    else -> LazyColumn(
                        modifier = Modifier.fillMaxSize(),
                        contentPadding = PaddingValues(bottom = 24.dp),
                    ) {
                        items(state.entries, key = { it.path }) { entry ->
                            FileRow(
                                entry = entry,
                                onClick = { vm.open(entry) },
                                onRename = { vm.showRename(entry) },
                                onDelete = { vm.showDelete(entry) },
                            )
                            HorizontalDivider()
                        }
                    }
                }
            }
        }
    }

    FileDialogs(dialog = state.dialog, vm = vm)
}

@Composable
private fun FileDialogs(dialog: Dialog?, vm: FilesViewModel) {
    when (dialog) {
        Dialog.NewFolder -> NameDialog(
            title = "New folder",
            label = "Folder name",
            initial = "",
            confirmText = "Create",
            onConfirm = vm::createFolder,
            onDismiss = vm::dismissDialog,
        )

        is Dialog.Rename -> NameDialog(
            title = "Rename",
            label = "Name",
            initial = dialog.entry.name,
            confirmText = "Rename",
            onConfirm = { vm.rename(dialog.entry, it) },
            onDismiss = vm::dismissDialog,
        )

        is Dialog.Delete -> AlertDialog(
            onDismissRequest = vm::dismissDialog,
            title = { Text("Delete ${dialog.entry.name}?") },
            text = {
                Text(
                    if (dialog.entry.isDirectory) {
                        "This deletes the folder and everything inside it."
                    } else {
                        "This can't be undone."
                    },
                )
            },
            confirmButton = { TextButton(onClick = { vm.delete(dialog.entry) }) { Text("Delete") } },
            dismissButton = { TextButton(onClick = vm::dismissDialog) { Text("Cancel") } },
        )

        null -> Unit
    }
}

@Composable
private fun FileRow(entry: FileEntry, onClick: () -> Unit, onRename: () -> Unit, onDelete: () -> Unit) {
    Row(
        modifier = Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        Icon(
            imageVector = if (entry.isDirectory) Icons.Filled.Folder else Icons.AutoMirrored.Filled.InsertDriveFile,
            contentDescription = null,
            tint = if (entry.isDirectory) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Column(modifier = Modifier.weight(1f)) {
            Text(
                text = entry.name,
                style = MaterialTheme.typography.bodyLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                text = subtitle(entry),
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
        }
        RowMenu(onRename = onRename, onDelete = onDelete)
    }
}

private fun subtitle(entry: FileEntry): String {
    val date = formatDate(entry.updatedAt)
    val size = entry.size
    return if (entry.isDirectory || size == null) date else "${formatSize(size)} · $date"
}

@Composable
private fun RowMenu(onRename: () -> Unit, onDelete: () -> Unit) {
    var expanded by remember { mutableStateOf(false) }
    Box {
        IconButton(onClick = { expanded = true }) {
            Icon(Icons.Filled.MoreVert, contentDescription = "More")
        }
        DropdownMenu(expanded = expanded, onDismissRequest = { expanded = false }) {
            DropdownMenuItem(
                text = { Text("Rename") },
                leadingIcon = { Icon(Icons.Filled.Edit, contentDescription = null) },
                onClick = {
                    expanded = false
                    onRename()
                },
            )
            DropdownMenuItem(
                text = { Text("Delete") },
                leadingIcon = { Icon(Icons.Filled.Delete, contentDescription = null) },
                onClick = {
                    expanded = false
                    onDelete()
                },
            )
        }
    }
}

@Composable
private fun NameDialog(
    title: String,
    label: String,
    initial: String,
    confirmText: String,
    onConfirm: (String) -> Unit,
    onDismiss: () -> Unit,
) {
    var text by remember { mutableStateOf(initial) }
    AlertDialog(
        onDismissRequest = onDismiss,
        title = { Text(title) },
        text = {
            OutlinedTextField(
                value = text,
                onValueChange = { text = it },
                label = { Text(label) },
                singleLine = true,
            )
        },
        confirmButton = {
            TextButton(onClick = { onConfirm(text) }, enabled = text.isNotBlank()) { Text(confirmText) }
        },
        dismissButton = { TextButton(onClick = onDismiss) { Text("Cancel") } },
    )
}

@Composable
private fun ErrorBanner(message: String, onDismiss: () -> Unit) {
    Surface(color = MaterialTheme.colorScheme.errorContainer, modifier = Modifier.fillMaxWidth()) {
        Row(
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                imageVector = Icons.Filled.ErrorOutline,
                contentDescription = null,
                tint = MaterialTheme.colorScheme.onErrorContainer,
            )
            Text(
                text = message,
                modifier = Modifier.weight(1f),
                color = MaterialTheme.colorScheme.onErrorContainer,
                style = MaterialTheme.typography.bodyMedium,
            )
            IconButton(onClick = onDismiss) {
                Icon(
                    imageVector = Icons.Filled.Close,
                    contentDescription = "Dismiss",
                    tint = MaterialTheme.colorScheme.onErrorContainer,
                )
            }
        }
    }
}

@Composable
private fun CenteredProgress() {
    Box(modifier = Modifier.fillMaxSize(), contentAlignment = Alignment.Center) {
        CircularProgressIndicator()
    }
}

@Composable
private fun EmptyFiles(onUpload: () -> Unit) {
    Box(modifier = Modifier.fillMaxSize().padding(32.dp), contentAlignment = Alignment.Center) {
        Column(
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Icon(
                imageVector = Icons.Filled.FolderOpen,
                contentDescription = null,
                modifier = Modifier.size(40.dp),
                tint = MaterialTheme.colorScheme.primary,
            )
            Text(text = "No files here", style = MaterialTheme.typography.titleMedium)
            Text(
                text = "Upload a file to get started.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            ExtendedFloatingActionButton(
                onClick = onUpload,
                icon = { Icon(Icons.Filled.Upload, contentDescription = null) },
                text = { Text("Upload") },
            )
        }
    }
}
