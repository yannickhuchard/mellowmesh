package io.mellowmesh.core

import org.junit.jupiter.api.Assertions.assertEquals
import org.junit.jupiter.api.Assertions.assertFalse
import org.junit.jupiter.api.Assertions.assertTrue
import org.junit.jupiter.api.Test

class EmailTest {
    private val ownerView = """
        {"settings":{"enabled":true,"address":"hub@gmail.com","username":"",
          "imap":{"host":"imap.gmail.com","port":993,"security":"tls"},
          "smtp":{"host":"smtp.gmail.com","port":465,"security":"tls"},
          "folder":"INBOX","poll_seconds":30,"notify_decisions":true,"notify_mentions":false,"trust_provider":true},
         "password_set":true,"password_from_env":false,
         "people":[{"address":"lea@studio.fr","person":"human://lea"}],
         "status":{"last_check":"2026-11-22T08:00:00Z","last_error":null,"received":3,"ignored":1,"sent":4},
         "future_field":1}
    """.trimIndent()

    @Test
    fun readsTheOwnersView() {
        val v = MeshJson.decodeFromString(EmailView.serializer(), ownerView)
        assertTrue(v.configured)
        assertTrue(v.settings.enabled)
        assertEquals("imap.gmail.com", v.settings.imap.host)
        assertEquals(30, v.settings.pollSeconds)
        assertEquals("human://lea", v.people.single().person)
        assertEquals(4, v.status.sent)
        // Someone who is not the owner gets another shape: it still reads.
        val other = MeshJson.decodeFromString(EmailView.serializer(), """{"enabled":true,"can_email":["human://lea"]}""")
        assertFalse(other.configured)
    }

    @Test
    fun aPatchCarriesOnlyWhatChanges() {
        val json = MeshJson.encodeToString(EmailPatch.serializer(), EmailPatch(enabled = true))
        assertEquals("""{"enabled":true}""", json)
        val setup = MeshJson.encodeToString(
            EmailPatch.serializer(),
            EmailPatch(address = "hub@gmail.com", password = "p", ownerAddress = "lea@studio.fr"),
        )
        assertEquals("""{"address":"hub@gmail.com","password":"p","owner_address":"lea@studio.fr"}""", setup)
        val remove = MeshJson.encodeToString(EmailPersonChange.serializer(), EmailPersonChange("lea@studio.fr", remove = true))
        assertEquals("""{"address":"lea@studio.fr","remove":true}""", remove)
    }

    @Test
    fun addressesAreChecked() {
        assertTrue(looksLikeEmail("lea@studio.fr"))
        assertTrue(looksLikeEmail(" a.b+mesh@sub.example.co "))
        assertFalse(looksLikeEmail("lea"))
        assertFalse(looksLikeEmail("lea@localhost"))
        assertFalse(looksLikeEmail("lea@studio.fr, eve@x.com"))
        assertFalse(looksLikeEmail("<lea@studio.fr>"))
        assertFalse(looksLikeEmail("a@b@c.fr"))
        assertFalse(looksLikeEmail("@studio.fr"))
    }
}
