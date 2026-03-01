package me.batashev.friday.bridge

data class SnapshotCounts(
    val conversations: Int,
    val notes: Int,
)

expect object FridayBridge {
    fun evaluateJs(source: String): Result<String>
    fun loadSnapshotCounts(databasePath: String): Result<SnapshotCounts>
}
