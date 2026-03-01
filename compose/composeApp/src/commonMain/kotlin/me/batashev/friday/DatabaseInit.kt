package me.batashev.friday

import me.batashev.friday.bridge.FridayBridge
import me.batashev.friday.bridge.SnapshotCounts

data class DatabaseInitState(
    val path: String,
    val counts: SnapshotCounts?,
    val error: String?,
)

fun initializeFridayDatabase(): DatabaseInitState {
    val databasePath = defaultFridayDatabasePath()

    val mkdirResult = createDirectoryRecursively(parentDirectory(databasePath))
    if (mkdirResult.isFailure) {
        return DatabaseInitState(
            path = databasePath,
            counts = null,
            error = "Could not create database directory: ${mkdirResult.exceptionOrNull()?.message}",
        )
    }

    val snapshotResult = FridayBridge.loadSnapshotCounts(databasePath)
    return snapshotResult.fold(
        onSuccess = { counts ->
            DatabaseInitState(
                path = databasePath,
                counts = counts,
                error = null,
            )
        },
        onFailure = { error ->
            DatabaseInitState(
                path = databasePath,
                counts = null,
                error = "CoreFriday init failed: ${error.message}",
            )
        }
    )
}

private fun parentDirectory(path: String): String =
    path.substringBeforeLast('/', missingDelimiterValue = path)

expect fun defaultFridayDatabasePath(): String
expect fun createDirectoryRecursively(path: String): Result<Unit>
