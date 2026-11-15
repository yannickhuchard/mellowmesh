package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertNull
import org.junit.jupiter.api.Test

class MiniMarkdownTest {
    @Test
    fun inlineStyles() {
        val spans = MiniMarkdown.parse("**Green** on `main` and *soon* done")
        assertEquals(MdSpan("Green", bold = true), spans[0])
        assertEquals(MdSpan(" on "), spans[1])
        assertEquals(MdSpan("main", code = true), spans[2])
        assertEquals(MdSpan(" and "), spans[3])
        assertEquals(MdSpan("soon", italic = true), spans[4])
        assertEquals(MdSpan(" done"), spans[5])
    }

    @Test
    fun linksOnlyForHttp() {
        val spans = MiniMarkdown.parse("see [docs](https://example.com/a) or [bad](ftp://x) <https://e.org>")
        assertEquals("https://example.com/a", spans.first { it.text == "docs" }.url)
        // An unsafe link keeps its label as plain text (merged with its neighbours).
        assertNull(spans.firstOrNull { it.url?.startsWith("ftp") == true })
        assertEquals("see docs or bad https://e.org", spans.joinToString("") { it.text })
        assertEquals("https://e.org", spans.last().url)
    }

    @Test
    fun headingsBulletsAndPlain() {
        assertEquals("Title\n• one\n• two", MiniMarkdown.plain("# Title\n- one\n* two"))
        assertEquals(true, MiniMarkdown.parse("## Hi").single().bold)
        assertEquals("snake_case_name stays", MiniMarkdown.plain("snake_case_name stays"))
        assertEquals("a < b", MiniMarkdown.plain("a < b"))
        assertEquals("*literal*", MiniMarkdown.plain("\\*literal\\*"))
    }
}

class GraphScopeTest {
    @Test
    fun parsesScopes() {
        assertEquals("all", GraphScope.parse(null).query)
        assertEquals("mine", GraphScope.parse("mine").query)
        assertEquals("agent:agent://y/coder", GraphScope.parse("agent:agent://y/coder").query)
        assertEquals("team:home", GraphScope.parse("team:home").query)
        assertEquals("all", GraphScope.parse("team:").query)
    }
}
