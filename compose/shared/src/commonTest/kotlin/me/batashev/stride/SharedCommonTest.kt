package me.batashev.stride

import kotlinx.serialization.json.Json
import me.batashev.stride.data.EventKind
import me.batashev.stride.data.ThreadEvent
import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertTrue

class SharedCommonTest {

    private val json = Json { ignoreUnknownKeys = true; isLenient = true; explicitNulls = false }

    @Test
    fun decodesAgentDelta() {
        val event = json.decodeFromString<ThreadEvent>(
            """{"seq":4,"thread_id":"t1","run_id":"r1","kind":{"type":"AgentDelta","content":"Hi"}}""",
        )
        assertEquals(4, event.seq)
        assertEquals("t1", event.threadId)
        val kind = event.kind
        assertTrue(kind is EventKind.AgentDelta)
        assertEquals("Hi", kind.content)
    }

    @Test
    fun decodesSnapshotWithPendingApproval() {
        val event = json.decodeFromString<ThreadEvent>(
            """{"seq":0,"thread_id":"t1","kind":{"type":"Snapshot","status":"running",
               "pending_approval":{"approval_id":"a1","message":"Run rm?"}}}""",
        )
        val kind = event.kind
        assertTrue(kind is EventKind.Snapshot)
        assertEquals("running", kind.status)
        assertEquals("a1", kind.pendingApproval?.approvalId)
    }

    @Test
    fun decodesRunFinishedObject() {
        val event = json.decodeFromString<ThreadEvent>(
            """{"seq":9,"thread_id":"t1","kind":{"type":"RunFinished"}}""",
        )
        assertTrue(event.kind is EventKind.RunFinished)
    }
}
