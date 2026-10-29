package io.virgoland.vsmedia.api;

import org.junit.jupiter.api.Test;
import java.net.URI;
import java.util.ArrayList;
import java.util.List;
import java.util.UUID;
import static org.junit.jupiter.api.Assertions.*;

class ResourcePackStackTest {
    static ResourcePackDelivery layer(String id, String sha1) {
        return new ResourcePackDelivery(id, UUID.randomUUID(),
            URI.create("https://packs.example.test/" + id + ".zip"), sha1, 1024);
    }
    @Test
    void snapshotsInputAndPreservesLayerOrder() {
        var base=layer("base","a".repeat(40));
        var overlay=layer("overlay","b".repeat(40));
        var mutable=new ArrayList<>(List.of(base,overlay));
        var stack=new ResourcePackStack(mutable);
        mutable.clear();
        assertEquals(List.of(base,overlay),stack.layers());
        assertThrows(UnsupportedOperationException.class, () ->stack.layers().clear());
    }
    @Test
    void rejectsMissingDuplicateAndExcessiveLayers() {
        var base=layer("base","a".repeat(40));
        assertThrows(IllegalArgumentException.class, () ->
            new ResourcePackStack(List.of()));
        assertThrows(IllegalArgumentException.class, () ->
            new ResourcePackStack(List.of(base,base)));
        assertThrows(IllegalArgumentException.class, () ->
            new ResourcePackStack(java.util.Collections.nCopies(9,base)));
    }
}
