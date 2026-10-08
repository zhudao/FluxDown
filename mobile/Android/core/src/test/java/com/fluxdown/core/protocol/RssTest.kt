package com.fluxdown.core.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RssTest {
    @Test
    fun sizeLiteralParsesAndRoundTrips() {
        assertEquals(200L shl 20, RssSizeLiteral.parse("200M"))
        assertEquals((1.5 * (1L shl 30)).toLong(), RssSizeLiteral.parse(" 1.5 GB "))
        assertEquals(1024L, RssSizeLiteral.parse("1024"))
        assertNull(RssSizeLiteral.parse("1.5.2G"))
        assertNull(RssSizeLiteral.parse("-1M"))
        assertEquals(0L, RssSizeLiteral.field(" "))
        assertEquals("2G", RssSizeLiteral.format(2L shl 30))
        assertEquals("1500", RssSizeLiteral.format(1500))
        assertEquals("", RssSizeLiteral.format(0))
    }

    @Test
    fun maxPerFetchAcceptsOnlyOneToHundred() {
        assertEquals(1, parseRssMaxPerFetch("1"))
        assertEquals(100, parseRssMaxPerFetch(" 100 "))
        assertNull(parseRssMaxPerFetch("0"))
        assertNull(parseRssMaxPerFetch("101"))
        assertNull(parseRssMaxPerFetch("1.5"))
    }

    @Test
    fun detailKeepsUnknownAndRuntimeFieldsOnWriteBack() {
        val raw = Json.parse("""{"sourceId":"s1","url":"https://a/feed","providerId":"bili","providerConfig":"{}","failCount":3,"unreadCount":2,"maxPerFetch":5}""")
        val d = RssSourceDetail.fromJson(raw)!!
        val out = d.copy(name = "N").toJson()
        assertEquals("bili", out.str("providerId"))
        assertEquals(3, out.int("failCount"))
        assertEquals("N", out.str("name"))
        assertEquals(5, out.int("maxPerFetch"))
        assertNull(RssSourceDetail.fromJson(Json.parse("""{"sourceId":"x"}""")))
        // 新建：缺省内置 provider
        assertEquals("rss", RssSourceDetail(url = "u").toJson().str("providerId"))
    }
}
