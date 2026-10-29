package io.virgoland.vsmedia.api;

import org.junit.jupiter.api.Test;

import java.net.URI;
import java.util.UUID;

import static org.junit.jupiter.api.Assertions.*;

class ResourcePackDeliveryTest {
    @Test
    void providesMinecraftSha1InExactlyTwentyBytes() {
        var delivery = new ResourcePackDelivery("virgoland", UUID.randomUUID(),
                URI.create("https://packs.example.test/abc.zip"),
                "0123456789abcdef0123456789abcdef01234567", 12345);
        assertEquals(20, delivery.minecraftSha1Bytes().length);
    }
    @Test
    void refusesInsecureOrMalformedResourcePackOffers() {
        UUID id=UUID.randomUUID();
        assertThrows(IllegalArgumentException.class, () ->
                new ResourcePackDelivery("demo",id,URI.create("http://example.test/pack.zip"),
                        "a".repeat(40),1));
        assertThrows(IllegalArgumentException.class, () ->
                new ResourcePackDelivery("demo",id,URI.create("https://example.test/pack.zip"),
                        "A".repeat(40),1));
        assertThrows(IllegalArgumentException.class, () ->
                new ResourcePackDelivery("demo",id,URI.create("https://example.test/?secret=abc"),
                        "a".repeat(40),1));
    }
}
