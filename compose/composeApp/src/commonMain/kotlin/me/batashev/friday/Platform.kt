package me.batashev.friday

interface Platform {
    val name: String
}

expect fun getPlatform(): Platform