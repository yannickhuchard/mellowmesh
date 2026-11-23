package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class GroupChatTest {
    private val group = MellowGroup("g_move", "Move team", listOf("agent://karim/reviewer", "agent://acme/movers"))
    private val reviewer = GroupMember("agent://karim/reviewer", "Reviewer", "home", "_agent.karim.reviewer", "human://karim")
    private val movers = GroupMember("agent://acme/movers", "Movers", "acme", "_agent.acme.movers", "human://karim-phone")
    private val members = listOf(reviewer, movers)
    private val dir = Directory(mapOf(reviewer.id to "Reviewer", movers.id to "Movers"))

    private fun ts(s: Int) = Time.formatMs(1_800_000_000_000L + s * 1_000L)

    private fun said(m: GroupMember, id: String, text: String, at: Int, turn: String, to: List<GroupMember> = members, ho: String? = null, from: GroupMember? = null) =
        MemberMessage(
            m,
            Message(
                id = id, topic = m.conversationId + ".inbox", from = m.me, timestamp = ts(at), body = text,
                payload = GroupChat.payload(group, members, turn, to, emptyList(), ho, from),
            ),
        )

    private fun answer(m: GroupMember, id: String, text: String, at: Int, parent: String) =
        MemberMessage(m, Message(id = id, topic = m.conversationId + ".outbox", from = m.id, timestamp = ts(at), body = text, parentId = parent))

    @Test
    fun one_line_said_to_two_machines_reads_as_one_line_and_two_answers() {
        val read = listOf(
            said(reviewer, "a1", "Who books the van?", 0, "t1"),
            said(movers, "b1", "Who books the van?", 0, "t1"),
            answer(reviewer, "a2", "Not me — @Movers does vans.", 3, "a1"),
            answer(movers, "b2", "We can, Saturday 9:00.", 4, "b1"),
            // A one-to-one line with Movers is not part of the group.
            MemberMessage(movers, Message(id = "b9", topic = "_agent.acme.movers.inbox", from = movers.me, timestamp = ts(5), body = "private")),
        )
        val items = GroupChat.thread(group, members, read, dir)
        assertEquals(3, items.size)
        val first = items[0] as GroupSaid
        assertTrue(first.message.isMine && first.member == null && first.toNames.isEmpty())
        assertEquals(listOf("Reviewer", "Movers"), items.drop(1).map { (it as GroupSaid).member?.name })
    }

    @Test
    fun an_answer_naming_another_member_is_handed_over_once() {
        val read = mutableListOf(
            said(reviewer, "a1", "Who books the van?", 0, "t1", to = listOf(reviewer)),
            answer(reviewer, "a2", "Not me — @movers does vans.", 3, "a1"),
        )
        var items = GroupChat.thread(group, members, read, dir)
        val jobs = GroupChat.handOvers(items, members)
        assertEquals(listOf("Movers"), jobs.map { it.to.name })
        assertTrue(GroupChat.waitingOn(items, members, 1_800_000_010_000L).isEmpty(), "Reviewer, the only one asked, answered")

        // Handed over: the next read shows the hand-over and asks nothing more.
        read += said(movers, "b1", GroupChat.handOverText(jobs[0].answer), 4, "t2", to = listOf(movers), ho = "a2", from = reviewer)
        items = GroupChat.thread(group, members, read, dir)
        val ho = items.filterIsInstance<GroupHandOver>().single()
        assertEquals("Reviewer" to "Movers", ho.fromName to ho.toName)
        assertTrue(GroupChat.handOvers(items, members).isEmpty())
        assertEquals(listOf("Movers"), GroupChat.waitingOn(items, members, 1_800_000_010_000L).map { it.name })

        read += answer(movers, "b2", "Booked: Saturday 9:00.", 6, "b1")
        items = GroupChat.thread(group, members, read, dir)
        assertTrue(GroupChat.waitingOn(items, members, 1_800_000_010_000L).isEmpty())
    }

    @Test
    fun hand_overs_stop_after_a_few_per_turn() {
        val read = mutableListOf(said(reviewer, "a1", "Go", 0, "t1"))
        for (i in 1..6) read += answer(reviewer, "a$i$i", "@Movers again $i", i, "a1")
        val items = GroupChat.thread(group, members, read, dir)
        assertEquals(GroupChat.MAX_HANDOVERS, GroupChat.handOvers(items, members).size)
    }

    @Test
    fun mentions_need_the_whole_name_and_addressees_default_to_everyone() {
        assertEquals(listOf(movers), GroupChat.addressees("@Movers can you?", members))
        assertEquals(members, GroupChat.addressees("mail@Moversville.com", members))
        assertEquals(members, GroupChat.addressees("Hello all", members))
        assertEquals(emptyList<GroupMember>(), GroupChat.mentions("@Reviewer me", members, except = reviewer.id))
    }

    @Test
    fun group_lines_stay_out_of_the_one_to_one_thread() {
        val history = listOf(
            said(movers, "b1", "group line", 0, "t1").message,
            answer(movers, "b2", "group answer", 1, "b1").message,
            Message(id = "b3", topic = "_agent.acme.movers.inbox", from = movers.me, timestamp = ts(2), body = "just us"),
            Message(id = "b4", topic = "_agent.acme.movers.outbox", from = movers.id, timestamp = ts(3), body = "sure", parentId = "b3"),
        )
        val thread = Chat.thread("_agent.acme.movers", history, movers.me, dir)
        assertEquals(listOf("just us", "sure"), thread.map { it.body })
        val summary = Chat.summaries(history, movers.me, dir).single()
        assertEquals("sure", summary.lastMessagePreview)
    }

    @Test
    fun another_group_and_strangers_are_left_out() {
        val other = MellowGroup("g_other", "Other", group.members)
        val foreign = MemberMessage(
            reviewer,
            Message(id = "x1", topic = "_agent.karim.reviewer.inbox", from = reviewer.me, timestamp = ts(0), body = "hi",
                payload = GroupChat.payload(other, members, "tx", members, emptyList())),
        )
        // An answer by someone else than the member, to a group line, is not the member's.
        val read = listOf(
            foreign,
            said(reviewer, "a1", "Go", 1, "t1"),
            MemberMessage(reviewer, Message(id = "a2", topic = "_agent.karim.reviewer.outbox", from = "agent://karim/intruder", timestamp = ts(2), body = "me!", parentId = "a1")),
        )
        val items = GroupChat.thread(group, members, read, dir)
        assertEquals(1, items.size)
    }
}
