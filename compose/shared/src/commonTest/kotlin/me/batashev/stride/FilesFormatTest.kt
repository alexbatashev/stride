package me.batashev.stride

import me.batashev.stride.util.formatDate
import me.batashev.stride.util.formatSize
import kotlin.test.Test
import kotlin.test.assertEquals

class FilesFormatTest {

    @Test
    fun formatsByteSizes() {
        assertEquals("0 B", formatSize(0))
        assertEquals("512 B", formatSize(512))
        assertEquals("1 KB", formatSize(1024))
        assertEquals("1.5 KB", formatSize(1536))
        assertEquals("2.5 MB", formatSize(2_621_440))
        assertEquals("1 GB", formatSize(1_073_741_824))
    }

    @Test
    fun formatsEpochMillisAsDate() {
        // 2026-06-21T00:00:00Z = 1_782_000_000_000 ms
        assertEquals("Jun 21, 2026", formatDate(1_782_000_000_000L))
        // 1970-01-01T00:00:00Z
        assertEquals("Jan 1, 1970", formatDate(0L))
    }
}
