package me.batashev.friday.bridge

actual object FridayBridge {
    private val loaded: Boolean by lazy {
        runCatching {
            System.loadLibrary("FridayBridge")
            System.loadLibrary("FridayBridgeJNI")
        }.isSuccess
    }

    actual fun evaluateJs(source: String): Result<String> {
        if (!loaded) return Result.failure(IllegalStateException("Friday native bridge is not loaded"))
        return runCatching { FridayBridgeBindings.nativeEvaluateJs(source) ?: "" }
    }

    actual fun loadSnapshotCounts(databasePath: String): Result<SnapshotCounts> {
        if (!loaded) return Result.failure(IllegalStateException("Friday native bridge is not loaded"))

        return runCatching {
            val raw = FridayBridgeBindings.nativeLoadSnapshotCounts(databasePath) ?: "0,0"
            parseSnapshotCounts(raw)
        }
    }

    private fun parseSnapshotCounts(raw: String): SnapshotCounts {
        if (raw.startsWith("error:")) {
            error(raw.removePrefix("error:"))
        }

        val parts = raw.split(',')
        require(parts.size == 2) { "Invalid snapshot counts: $raw" }
        return SnapshotCounts(parts[0].toInt(), parts[1].toInt())
    }
}

private object FridayBridgeBindings {
    @JvmStatic external fun nativeEvaluateJs(source: String): String?
    @JvmStatic external fun nativeLoadSnapshotCounts(databasePath: String): String?
}
