package io.virgoland.vsmedia.plugin;

import io.virgoland.vsmedia.api.ResourcePackDelivery;
import org.junit.jupiter.api.Test;
import java.net.URI;
import java.util.List;
import static org.junit.jupiter.api.Assertions.*;

class RemotePackCatalogTest {
    private static final URI SOURCE = URI.create("https://packs.example.org/virgoland/current.json");
    private static final String HASH = "f".repeat(40);
    private static String json(String url, String status, int priority, long size) {
        return """
            {"schema":"vspack-publication-v1","status":"%s","packs":[{
              "logical_id":"audio-01",
              "uuid":"c6e821b7-293b-5532-b065-aab9d38c1367",
              "url":"%s","sha1":"%s",
              "bytes":%d,"priority":%d
            }]}
            """.formatted(status,url,HASH,size,priority);
    }

    @Test
    void acceptsImmutablePublicationAndPreservesOrder() {
        List<ResourcePackDelivery> current = RemotePackCatalog.parseManifest(
            json("https://packs.example.org/virgoland/versions/audio.zip",
                "uploaded_to_r2", 0, 95L * 1024 * 1024),SOURCE);
        assertEquals(1,current.size());
        assertEquals("audio-01",current.getFirst().logicalId());
        assertEquals(95L * 1024 * 1024,current.getFirst().sizeBytes());
        assertThrows(UnsupportedOperationException.class, () -> current.clear());
    }

    @Test
    void rejectsUnpublishedMixedHostAndBadOrder() {
        assertThrows(IllegalArgumentException.class, () -> RemotePackCatalog.parseManifest(
            json("https://packs.example.org/demo.zip","built_not_published",0,123),SOURCE));
        assertThrows(IllegalArgumentException.class, () -> RemotePackCatalog.parseManifest(
            json("https://external.example.org/demo.zip","uploaded_to_r2",0,123),SOURCE));
        assertThrows(IllegalArgumentException.class, () -> RemotePackCatalog.parseManifest(
            json("https://packs.example.org/demo.zip","uploaded_to_r2",2,123),SOURCE));
    }

    @Test
    void rejectsNonHttpsManifestAndInvalidPackSizes() {
        assertThrows(IllegalArgumentException.class, () -> RemotePackCatalog.validateManifestUrl(
            URI.create("http://packs.example.org/current.json")));
        assertThrows(IllegalArgumentException.class, () -> RemotePackCatalog.parseManifest(
            json("https://packs.example.org/demo.zip","uploaded_to_r2",0,251L * 1024 * 1024),SOURCE));
    }

    @Test
    void cdnMustServeEveryImmutableZipBeforeManifestCanGoLive() throws Exception {
        assertDoesNotThrow(() -> RemotePackCatalog.validateZipAvailability(200, 184320, 184320));
        assertThrows(java.io.IOException.class,
            () -> RemotePackCatalog.validateZipAvailability(404, -1, 184320));
        assertThrows(java.io.IOException.class,
            () -> RemotePackCatalog.validateZipAvailability(302, -1, 184320));
        assertThrows(java.io.IOException.class,
            () -> RemotePackCatalog.validateZipAvailability(200, 7, 184320));
        assertThrows(java.io.IOException.class,
            () -> RemotePackCatalog.validateZipAvailability(200, -1, 184320));
    }

    @Test
    void rejectsMalformedAndEmptyDocuments() {
        assertThrows(RuntimeException.class,
            () -> RemotePackCatalog.parseManifest("{}", SOURCE));
        assertThrows(IllegalArgumentException.class,
            () -> RemotePackCatalog.parseManifest(
                "{\"schema\":\"vspack-publication-v1\",\"status\":\"uploaded_to_r2\",\"packs\":[]}", SOURCE));
    }
}
