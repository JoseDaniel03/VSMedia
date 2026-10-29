package io.virgoland.vsmedia.api;

import org.junit.jupiter.api.Test;

import java.net.URI;
import java.util.UUID;
import java.util.Set;
import static org.junit.jupiter.api.Assertions.*;

class ResourcePackSessionCoordinatorTest {
    private static final String FIRST = "a".repeat(40);
    private static final String UPDATED = "b".repeat(40);
    private static final UUID PACK_ID = UUID.fromString("9d19a73d-4a73-456e-bce3-10e846662801");

    private static ResourcePackDelivery pack(String hash) {
        return new ResourcePackDelivery("virgoland", PACK_ID,
            URI.create("https://packs.example.test/" + hash + ".zip"), hash, 8192);
    }

    @Test
    void suppressesDuplicateOffersAndAllowsNewRevisionImmediately() {
        var coordinator=new ResourcePackSessionCoordinator();
        var player=UUID.randomUUID();
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(player,pack(FIRST),false));
        assertEquals(ResourcePackSessionCoordinator.Decision.IN_PROGRESS,
            coordinator.prepare(player,pack(FIRST),false));
        assertTrue(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.ACCEPTED));
        assertTrue(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.DOWNLOADED));
        assertFalse(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.ACCEPTED));
        assertTrue(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.SUCCESSFULLY_LOADED));
        assertEquals(ResourcePackSessionCoordinator.Decision.ALREADY_APPLIED,
            coordinator.prepare(player,pack(FIRST),false));
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(player,pack(UPDATED),false));
        assertEquals(UPDATED,coordinator.snapshot(player,PACK_ID).orElseThrow().sha1Hex());
        coordinator.onQuit(player);
        assertEquals(0,coordinator.sessionCount());
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(player,pack(UPDATED),false));
    }

    @Test
    void declinesAndFailuresAreNotRetriedAutomatically() {
        var coordinator=new ResourcePackSessionCoordinator();
        var player=UUID.randomUUID();
        coordinator.prepare(player,pack(FIRST),false);
        coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.DECLINED);
        assertEquals(ResourcePackSessionCoordinator.Decision.PREVIOUSLY_DECLINED,
            coordinator.prepare(player,pack(FIRST),false));
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(player,pack(FIRST),true));
        coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.FAILED_DOWNLOAD);
        assertEquals(ResourcePackSessionCoordinator.Decision.PREVIOUSLY_FAILED,
            coordinator.prepare(player,pack(FIRST),false));
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(player,pack(UPDATED),false));
    }

    @Test
    void ignoresUnknownOrLateEventsAndTransportFailures() {
        var coordinator=new ResourcePackSessionCoordinator();
        var player=UUID.randomUUID();
        assertFalse(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.ACCEPTED));
        coordinator.prepare(player,pack(FIRST),false);
        assertFalse(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.UNKNOWN));
        coordinator.sendFailed(player,PACK_ID,UPDATED);
        assertEquals(ResourcePackSessionCoordinator.Decision.IN_PROGRESS,
            coordinator.prepare(player,pack(FIRST),false));
        coordinator.sendFailed(player,PACK_ID,FIRST);
        assertEquals(ResourcePackSessionCoordinator.Decision.PREVIOUSLY_FAILED,
            coordinator.prepare(player,pack(FIRST),false));
        assertFalse(coordinator.onClientStatus(player,PACK_ID,
            ResourcePackSessionCoordinator.ClientStatus.SUCCESSFULLY_LOADED));
    }

    @Test
    void handlesMultiplePackSlotsAndTracksPerPlayerLimits() {
        var coordinator=new ResourcePackSessionCoordinator();
        var player=UUID.randomUUID();
        var second=new ResourcePackDelivery("cosmetics",UUID.randomUUID(),
            URI.create("https://packs.example.test/c.zip"),"c".repeat(40),7);
        coordinator.prepare(player,pack(FIRST),false);
        coordinator.prepare(player,second,false);
        assertEquals(2,coordinator.sessionCount());
        assertEquals(ResourcePackSessionCoordinator.ClientStatus.OFFERED,
            coordinator.snapshot(player,second.packId()).orElseThrow().status());
        coordinator.onQuit(player);
        assertEquals(0,coordinator.sessionCount());
    }

    @Test
    void enforcesBoundedSessionsWithoutEvictingLivePlayers() {
        var coordinator=new ResourcePackSessionCoordinator();
        for (int i=0;i<ResourcePackSessionCoordinator.MAX_SESSIONS;i++) {
            UUID player=new UUID(0,i+1);
            assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
                coordinator.prepare(player,pack(FIRST),false));
        }
        assertEquals(ResourcePackSessionCoordinator.Decision.AT_CAPACITY,
            coordinator.prepare(UUID.randomUUID(),pack(FIRST),false));
        coordinator.onQuit(new UUID(0,1));
        assertEquals(ResourcePackSessionCoordinator.Decision.SEND,
            coordinator.prepare(UUID.randomUUID(),pack(FIRST),false));
    }

    @Test
    void reconcilesOnlyItsOwnObsoletePackSlots() {
        var coordinator=new ResourcePackSessionCoordinator();
        UUID player=UUID.randomUUID();
        UUID foreignPack=UUID.randomUUID();
        var base=pack(FIRST);
        var overlay=new ResourcePackDelivery("overlay",UUID.randomUUID(),
            URI.create("https://packs.example.test/overlay.zip"),UPDATED,8192);
        coordinator.prepare(player,base,false);
        coordinator.prepare(player,overlay,false);
        assertEquals(Set.of(overlay.packId()),
            coordinator.obsolete(player,Set.of(base.packId(),foreignPack)));
        assertTrue(coordinator.removed(player,overlay.packId()));
        assertFalse(coordinator.removed(player,overlay.packId()));
        assertTrue(coordinator.obsolete(player,Set.of(base.packId())).isEmpty());
        assertEquals(1,coordinator.sessionCount());
    }

    @Test
    void unknownPaperStatusesDoNotCrashTheBridge() {
        assertEquals(ResourcePackSessionCoordinator.ClientStatus.UNKNOWN,
            ResourcePackSessionCoordinator.ClientStatus.fromPaperStatus("SOMETHING_NEW"));
        assertEquals(ResourcePackSessionCoordinator.ClientStatus.DOWNLOADED,
            ResourcePackSessionCoordinator.ClientStatus.fromPaperStatus("DOWNLOADED"));
        assertEquals(ResourcePackSessionCoordinator.ClientStatus.UNKNOWN,
            ResourcePackSessionCoordinator.ClientStatus.fromPaperStatus(null));
    }
}
