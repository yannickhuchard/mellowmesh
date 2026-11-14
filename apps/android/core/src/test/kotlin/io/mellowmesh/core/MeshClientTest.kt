package io.mellowmesh.core

import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import okhttp3.Response
import okhttp3.WebSocket
import okhttp3.WebSocketListener
import okhttp3.mockwebserver.MockResponse
import okhttp3.mockwebserver.MockWebServer
import org.junit.jupiter.api.AfterEach
import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Assertions.assertThrows
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.BeforeEach
import org.junit.jupiter.api.Test
import java.util.concurrent.CountDownLatch
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

class MeshClientTest {
    private lateinit var server: MockWebServer
    private lateinit var client: MeshClient

    @BeforeEach
    fun setUp() {
        server = MockWebServer()
        server.start()
        client = MeshClient(MeshConfig(server.url("/").toString(), "mm_secret", "human://yannick", "Pixel"))
    }

    @AfterEach
    fun tearDown() {
        server.shutdown()
    }

    private fun json(body: String, code: Int = 200) =
        MockResponse().setResponseCode(code).setHeader("Content-Type", "application/json").setBody(body)

    @Test
    fun whoamiSendsBearerToken() {
        server.enqueue(json("""{"principal":"human://yannick","kind":"human","is_owner":true,"remote":false,"extra":1}"""))
        val me = client.whoami()
        assertEquals("human://yannick", me.principal)
        assertTrue(me.isOwner)
        val req = server.takeRequest()
        assertEquals("/whoami", req.path)
        assertEquals("Bearer mm_secret", req.getHeader("Authorization"))
        assertFalse(client.config.toString().contains("mm_secret"))
    }

    @Test
    fun networkSpacesAndTeams() {
        server.enqueue(json("""{"network":{"name":"Mesh","description":"","default_access":"open","visitor_write":false},
            "spaces":[{"id":"family","name":"Family","patterns":["_chat.group.family.**"],"access":"closed","members":["human://alice"],"teams":["home"]}],
            "teams":[{"id":"home","name":"Home","color":"#10B981","members":[{"principal":"human://alice","role":"member"}]}]}"""))
        val net = client.network()
        assertEquals(Access.OPEN, net.network.defaultAccess)
        assertEquals(listOf("_chat.group.family.**"), net.spaces.single().patterns)
        assertEquals("_team.home.chat", net.teams.single().roomTopic)
        assertEquals("/network", server.takeRequest().path)

        server.enqueue(json(""))
        client.updateNetwork(NetworkPatch(defaultAccess = Access.CLOSED))
        val put = server.takeRequest()
        assertEquals("PUT", put.method)
        assertEquals("""{"default_access":"closed"}""", put.body.readUtf8())

        server.enqueue(json("""{"id":"family","name":"Family","access":"closed","created_at":"2026-01-01T00:00:00Z"}""", 201))
        val created = client.createSpace(Space("family", "Family", patterns = listOf("_chat.group.family.**")))
        assertEquals("2026-01-01T00:00:00Z", created.createdAt)
        val post = server.takeRequest()
        assertEquals("POST", post.method)
        assertEquals("/spaces", post.path)
        val sent = MeshJson.parseToJsonElement(post.body.readUtf8()).jsonObject
        assertEquals("family", sent["id"]!!.jsonPrimitive.content)
        assertEquals(null, sent["created_at"]) // server-set, not sent

        server.enqueue(MockResponse().setResponseCode(204))
        client.deleteSpace("family")
        val del = server.takeRequest()
        assertEquals("DELETE", del.method)
        assertEquals("/spaces/family", del.path)

        assertThrows(IllegalArgumentException::class.java) { client.deleteSpace("../etc") }

        server.enqueue(MockResponse().setResponseCode(204))
        client.removeTeamMember("home", "human://alice")
        assertEquals("/teams/home/members/human:%2F%2Falice", server.takeRequest().path)

        server.enqueue(json(""))
        client.addTeamMember("home", TeamMember("agent://y/coder", TeamRole.ADMIN))
        val add = server.takeRequest()
        assertEquals("/teams/home/members", add.path)
        assertEquals("""{"principal":"agent://y/coder","role":"admin"}""", add.body.readUtf8())
    }

    @Test
    fun historyAndPublish() {
        server.enqueue(json("""[{"id":"m1","topic":"_project.x","from":"agent://y/a","timestamp":"2026-06-06T13:02:24.123456789Z","content_type":"text/plain","body":"hi"}]"""))
        val h = client.history("_project.x.**", 5000)
        assertEquals(1, h.size)
        assertEquals(Time.parseMs("2026-06-06T13:02:24.123Z"), h[0].timestampMs)
        val req = server.takeRequest()
        assertEquals("/history?limit=1000&pattern=_project.x.**", req.path)

        server.enqueue(json("""{"id":"m2","topic":"_agent.y.coder.inbox","from":"human://yannick","timestamp":"2026-06-06T13:02:25Z","content_type":"text/plain","body":"hello"}"""))
        val stored = client.sendText("_agent.y.coder", "hello")
        assertEquals("m2", stored.id)
        val pub = server.takeRequest()
        assertEquals("/publish", pub.path)
        val body = MeshJson.parseToJsonElement(pub.body.readUtf8()).jsonObject
        assertEquals("_agent.y.coder.inbox", body["topic"]!!.jsonPrimitive.content)
        assertEquals("human://yannick", body["from"]!!.jsonPrimitive.content)
        assertEquals("", body["id"]!!.jsonPrimitive.content)
        assertEquals("Pixel", (body["headers"] as JsonObject)["x-mellowmesh-device"]!!.jsonPrimitive.content)
        assertNull(body["payload"])

        server.enqueue(json("{}"))
        client.sendUiAction("_team.home", "msg_9", "retry", "Retry")
        val ui = MeshJson.parseToJsonElement(server.takeRequest().body.readUtf8()).jsonObject
        assertEquals("_team.home.chat", ui["topic"]!!.jsonPrimitive.content)
        assertEquals(MeshUi.ACTION_CONTENT_TYPE, ui["content_type"]!!.jsonPrimitive.content)
        assertEquals("msg_9", ui["parent_id"]!!.jsonPrimitive.content)
        val act = ui["payload"]!!.jsonObject["ui_action"]!!.jsonObject
        assertEquals("retry", act["action_id"]!!.jsonPrimitive.content)

        assertThrows(IllegalArgumentException::class.java) { client.sendUiAction("_team.home", "m", "bad id", "x") }
        assertThrows(MeshException::class.java) { client.sendText("_task.x", "hi") }
    }

    @Test
    fun decisionsAndTasks() {
        server.enqueue(json("""[{"id":"d1","title":"Deploy?","question":"Now?","created_by":"agent://y/ops","required_decider":"human://y",
            "status":"requested","options":[{"id":"yes","label":"Yes","outcome":"approve"},{"id":"no","label":"No","outcome":"reject"}]}]"""))
        val ds = client.decisions()
        assertTrue(ds.single().isPending)
        assertEquals(DecisionOutcome.REJECT, ds.single().options[1].outcome)
        server.takeRequest()

        server.enqueue(json("""{"status":"approved"}"""))
        client.respondDecision("d1", "yes")
        val r = server.takeRequest()
        assertEquals("/decisions/d1/respond", r.path)
        assertEquals("""{"option_id":"yes"}""", r.body.readUtf8())
        assertThrows(IllegalArgumentException::class.java) { client.respondDecision("../x", "yes") }

        server.enqueue(json("""{"id":"t1","title":"Build","status":"in_progress","priority":"high","created_by":"human://y",
            "topics":[],"required_capabilities":[],"artifacts":[],"decisions":[],"depends_on":["t0"],"started_at":"2026-01-01T00:00:00Z"}"""))
        val t = client.updateTask("t1", TaskPatch(status = TaskStatus.IN_PROGRESS))!!
        assertEquals(listOf("t0"), t.dependsOn)
        val patch = server.takeRequest()
        assertEquals("PATCH", patch.method)
        assertEquals("/tasks/t1", patch.path)
        assertEquals("""{"status":"in_progress"}""", patch.body.readUtf8())

        server.enqueue(json("""{"now":"2026-01-01T01:00:00Z","lanes":[{"id":"agent://y/coder","kind":"agent","label":"Coder"}],
            "tasks":[{"id":"t1","title":"Build","status":"in_progress","lane":"agent://y/coder","blocked_by":[],"waiting_seconds":60,"running_seconds":120,"critical":true}],
            "edges":[{"from":"t0","to":"t1"}]}"""))
        val g = client.taskGraph(GraphScope.Agent("agent://y/coder"))
        assertEquals(120, g.tasks.single().runningSeconds)
        assertTrue(g.tasks.single().critical)
        assertEquals("/tasks/graph?scope=agent%3Aagent%3A%2F%2Fy%2Fcoder", server.takeRequest().path)

        server.enqueue(json("""[{"id":"agent://y/coder","name":"Coder","owner":"human://y","mode":"autonomous","capabilities":["code"]}]"""))
        assertEquals("Coder", client.agents().single().name)
    }

    @Test
    fun errorsAreMappedToPlainWords() {
        server.enqueue(MockResponse().setResponseCode(401).setBody("unauthorized"))
        val e = assertThrows(MeshException::class.java) { client.whoami() }
        assertEquals(MeshException.Kind.UNAUTHORIZED, e.kind)
        assertTrue(e.friendly.contains("Pair again"))

        server.enqueue(MockResponse().setResponseCode(403).setBody("no"))
        assertEquals(MeshException.Kind.FORBIDDEN, assertThrows(MeshException::class.java) { client.spaces() }.kind)

        server.enqueue(json("not json"))
        assertEquals(MeshException.Kind.PROTOCOL, assertThrows(MeshException::class.java) { client.teams() }.kind)

        val offline = MeshClient(MeshConfig("http://127.0.0.1:1", null))
        assertEquals(MeshException.Kind.NETWORK, assertThrows(MeshException::class.java) { offline.topics() }.kind)
    }

    @Test
    fun webSocketSubscribeDeliversMessages() {
        val serverSide = LinkedBlockingQueue<WebSocket>()
        server.enqueue(MockResponse().withWebSocketUpgrade(object : WebSocketListener() {
            override fun onOpen(webSocket: WebSocket, response: Response) {
                serverSide.add(webSocket)
                webSocket.send("""{"id":"m1","topic":"_task.t1.completed","from":"agent://y/coder","timestamp":"2026-01-01T00:00:00Z","content_type":"application/json","body":"","payload":{"id":"t1","title":"Build"}}""")
                webSocket.send("garbage")
                webSocket.send("""{"id":"m2","topic":"_project.x","from":"agent://y/coder","timestamp":"2026-01-01T00:00:01Z","content_type":"text/plain","body":"hi"}""")
            }
        }))
        val got = LinkedBlockingQueue<Message>()
        val opened = CountDownLatch(1)
        val closed = CountDownLatch(1)
        val sub = client.subscribe("**", object : LiveListener {
            override fun onOpen() = opened.countDown()
            override fun onMessage(message: Message) {
                got.add(message)
            }
            override fun onClosed(error: Throwable?) = closed.countDown()
        })
        assertTrue(opened.await(5, TimeUnit.SECONDS))
        assertEquals("m1", got.poll(5, TimeUnit.SECONDS)!!.id)
        assertEquals("m2", got.poll(5, TimeUnit.SECONDS)!!.id)
        val req = server.takeRequest()
        assertEquals("/ws?pattern=**&token=mm_secret", req.path)
        assertEquals("Bearer mm_secret", req.getHeader("Authorization"))
        serverSide.poll(5, TimeUnit.SECONDS)!!.close(1000, "bye")
        assertTrue(closed.await(5, TimeUnit.SECONDS))
        sub.close()
    }

    @Test
    fun backoffGrowsAndCaps() {
        val b = Backoff(1000, 8000)
        assertEquals(listOf(1000L, 2000L, 4000L, 8000L, 8000L), List(5) { b.nextDelayMs() })
        b.reset()
        assertEquals(1000L, b.nextDelayMs())
    }
}
