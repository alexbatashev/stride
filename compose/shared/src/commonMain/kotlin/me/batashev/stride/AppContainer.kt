package me.batashev.stride

import me.batashev.stride.data.FridayClient
import me.batashev.stride.data.Session

/** Process-wide singletons shared by every view model. */
class AppContainer(settings: Settings = createSettings()) {
    val session = Session(settings)
    val client = FridayClient(session)
}
