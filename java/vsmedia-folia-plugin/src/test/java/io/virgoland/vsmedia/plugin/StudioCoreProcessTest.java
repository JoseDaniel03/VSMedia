package io.virgoland.vsmedia.plugin;

import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

class StudioCoreProcessTest {
    @Test
    void restartBackoffIsBoundedAndGrowsWithFailures() {
        assertEquals(10_000L,StudioCoreProcess.restartDelayMillis(1));
        assertEquals(20_000L,StudioCoreProcess.restartDelayMillis(2));
        assertEquals(40_000L,StudioCoreProcess.restartDelayMillis(3));
        assertEquals(80_000L,StudioCoreProcess.restartDelayMillis(4));
        assertEquals(160_000L,StudioCoreProcess.restartDelayMillis(5));
        assertEquals(300_000L,StudioCoreProcess.restartDelayMillis(100));
    }
}
