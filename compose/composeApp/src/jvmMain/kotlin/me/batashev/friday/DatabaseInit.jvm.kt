package me.batashev.friday

import java.io.File

actual fun defaultFridayDatabasePath(): String {
    val appSupport = System.getProperty("user.home")
        ?.let { File(it, ".local/share") }
        ?: File(System.getProperty("java.io.tmpdir", "/tmp"))

    return File(File(appSupport, "Friday"), "db.sqlite").absolutePath
}

actual fun createDirectoryRecursively(path: String): Result<Unit> = runCatching {
    val directory = File(path)
    if (!directory.exists() && !directory.mkdirs()) {
        error("mkdirs() returned false for ${directory.absolutePath}")
    }
}
