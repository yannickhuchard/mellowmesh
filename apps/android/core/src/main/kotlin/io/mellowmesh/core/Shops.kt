package io.mellowmesh.core

/** Shops floors (design language: Popular · Study · Writing · Money · Work). */
enum class Floor(val title: String, val blurb: String, internal val keywords: List<String>) {
    POPULAR("Popular", "Helpers people use the most.", emptyList()),
    STUDY("Study", "Learn, revise, get quizzed.", listOf("study", "tutor", "learn", "teach", "quiz", "course", "research", "explain", "school", "education", "language", "translate")),
    WRITING("Writing", "Letters, reports, emails, stories.", listOf("writ", "draft", "edit", "summar", "email", "letter", "report", "blog", "copy", "proofread", "translat", "docs", "document")),
    MONEY("Money", "Budgets, invoices, cheaper tickets.", listOf("money", "financ", "budget", "invoice", "account", "tax", "pay", "bank", "price", "expense", "shop", "travel", "ticket")),
    WORK("Work", "Plans, code, shifts and projects.", listOf("work", "code", "coding", "review", "deploy", "ops", "build", "test", "plan", "schedule", "shift", "project", "task", "support", "data", "analy", "calendar", "meeting")),
    EVERYTHING("Everything", "All the helpers on your network.", emptyList()),
}

data class ShopWindow(
    val agent: AgentRegistration,
    val name: String,
    val accent: Accent,
    /** "Try saying…" examples derived from the capabilities. */
    val trySaying: List<String>,
    val conversationId: String?,
)

data class ShopFloor(val floor: Floor, val shops: List<ShopWindow>)

object Shops {
    private const val POPULAR_MAX = 6

    fun floorsOf(agent: AgentRegistration): Set<Floor> {
        val words = (agent.capabilities + agent.name + agent.id.substringAfterLast('/')).map { it.lowercase() }
        return Floor.entries.filter { f -> f.keywords.isNotEmpty() && f.keywords.any { k -> words.any { k in it } } }.toSet()
    }

    fun trySaying(agent: AgentRegistration): List<String> {
        val floors = floorsOf(agent)
        val out = ArrayList<String>()
        if (Floor.STUDY in floors) out += "Quiz me on chapter 3"
        if (Floor.WRITING in floors) out += "Write a short thank-you letter"
        if (Floor.MONEY in floors) out += "Find me a cheaper train ticket"
        if (Floor.WORK in floors) out += "Plan my shifts for next week"
        if (out.isEmpty()) out += "What can you do for me?"
        return out.take(2)
    }

    /**
     * Group the directory by floor. Popular lists my own helpers first, then
     * the rest, up to six. A helper can be on several floors. When no helper
     * matches any keyword floor, only Popular and Everything are shown.
     */
    fun floors(agents: List<AgentRegistration>, me: String?): List<ShopFloor> {
        val dir = Directory.of(agents)
        fun window(a: AgentRegistration) = ShopWindow(
            agent = a,
            name = dir.nameOf(a.id),
            accent = Identity.accentOf(a.id),
            trySaying = trySaying(a),
            conversationId = Conversations.agentConversationId(a.id),
        )
        val sorted = agents.sortedWith(
            compareByDescending<AgentRegistration> { Identity.samePrincipal(it.owner, me) }
                .thenByDescending { it.capabilities.size }
                .thenBy { dir.nameOf(it.id).lowercase() },
        )
        val out = ArrayList<ShopFloor>()
        if (sorted.isNotEmpty()) out += ShopFloor(Floor.POPULAR, sorted.take(POPULAR_MAX).map(::window))
        for (f in listOf(Floor.STUDY, Floor.WRITING, Floor.MONEY, Floor.WORK)) {
            val shops = sorted.filter { f in floorsOf(it) }.map(::window)
            if (shops.isNotEmpty()) out += ShopFloor(f, shops)
        }
        out += ShopFloor(Floor.EVERYTHING, sorted.sortedBy { dir.nameOf(it.id).lowercase() }.map(::window))
        return out
    }
}
