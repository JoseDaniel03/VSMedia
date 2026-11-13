package io.virgoland.vsmedia.plugin;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class RadioServiceTest {
    @Test void resolvesCommandStationIdsLikeTheWeb() {
        assertEquals("latinsabroso", RadioService.stationId("LatinSabroso"));
        assertEquals("radioromantica", RadioService.stationId("Radio Romántica"));
        assertEquals("", RadioService.stationId("../"));
        assertEquals("radio_123", RadioService.stationId("RADIO_123"));
        assertTrue(RadioService.stationId("a".repeat(120)).length()<=48);
    }

    @Test void lateSubscriberJoinsAlreadyPlayingFrames() {
        // 48 mono 16-bit samples per millisecond.
        assertEquals(48 * 20, RadioService.sampleOffset(0,1000,1020,10000));
        assertEquals(48 * 3025, RadioService.sampleOffset(3000,1000,1025,500000));
        assertEquals(5000, RadioService.sampleOffset(900000,1000,999000,5000));
        // Bad client timestamp must not rewind the stream.
        assertEquals(2400, RadioService.sampleOffset(50,3000,1000,99999));
    }
}
