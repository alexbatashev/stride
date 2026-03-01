package me.batashev.friday

import java.io.File

actual fun defaultFridayDatabasePath(): String {
    val appSupportBase = System.getProperty("user.home")
        ?: System.getProperty("java.io.tmpdir", "/tmp")
    val appSupport = File(appSupportBase, "files")

    return File(File(appSupport, "Friday"), "db.sqlite").absolutePath
}

actual fun createDirectoryRecursively(path: String): Result<Unit> = runCatching {
    val directory = File(path)
    if (!directory.exists() && !directory.mkdirs()) {
        error("mkdirs() returned false for ${directory.absolutePath}")
    }
}
