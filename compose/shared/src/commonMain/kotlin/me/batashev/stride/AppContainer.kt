package me.batashev.stride

import me.batashev.stride.data.StrideClient
import me.batashev.stride.data.Session

/** Process-wide singletons shared by every view model. */
class AppContainer(settings: Settings = createSettings()) {
    val session = Session(settings)
    val client = StrideClient(session)
}
