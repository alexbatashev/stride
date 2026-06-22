package me.batashev.stride.util

private val MONTHS = listOf(
    "Jan", "Feb", "Mar", "Apr", "May", "Jun",
    "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
)

/** Human-readable byte count, e.g. `2.5 MB`. */
fun formatSize(bytes: Long): String {
    if (bytes < 1024) return "$bytes B"
    val units = listOf("KB", "MB", "GB", "TB")
    var value = bytes.toDouble() / 1024
    var unit = 0
    while (value >= 1024 && unit < units.lastIndex) {
        value /= 1024
        unit++
    }
    val whole = value.toLong()
    val tenths = ((value - whole) * 10).toLong()
    val number = if (tenths == 0L) "$whole" else "$whole.$tenths"
    return "$number ${units[unit]}"
}

/** Abbreviated UTC date for a millisecond epoch timestamp, e.g. `Jun 21, 2026`. */
fun formatDate(epochMs: Long): String {
    val days = epochMs.floorDiv(86_400_000L)
    val (year, month, day) = civilFromDays(days)
    return "${MONTHS[month - 1]} $day, $year"
}

/** Converts days since 1970-01-01 to a (year, month, day) triple (Howard Hinnant's algorithm). */
private fun civilFromDays(days: Long): Triple<Int, Int, Int> {
    val z = days + 719468
    val era = (if (z >= 0) z else z - 146096) / 146097
    val doe = z - era * 146097
    val yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365
    val year = yoe + era * 400
    val doy = doe - (365 * yoe + yoe / 4 - yoe / 100)
    val mp = (5 * doy + 2) / 153
    val day = (doy - (153 * mp + 2) / 5 + 1).toInt()
    val month = (if (mp < 10) mp + 3 else mp - 9).toInt()
    return Triple((year + if (month <= 2) 1 else 0).toInt(), month, day)
}
