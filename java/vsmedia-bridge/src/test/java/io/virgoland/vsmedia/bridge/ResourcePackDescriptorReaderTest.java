package io.virgoland.vsmedia.bridge;

import io.virgoland.vsmedia.api.ResourcePackDelivery;
import org.junit.jupiter.api.Test;

import java.io.IOException;
import java.nio.charset.StandardCharsets;
import static org.junit.jupiter.api.Assertions.*;

class ResourcePackDescriptorReaderTest {
    private static final String HASH = "0123456789abcdef".repeat(4);

    private static byte[] sample(String url) {
        String json = """
            {"logical_id":"virgoland","uuid":"b148bbd9-9223-5ae5-9c3d-1f76894ce600",
             "sha1":"0123456789abcdef0123456789abcdef01234567",
             "sha256":"%s", "size_bytes":1234,
             "filename":"%s.zip", "url":"%s"}
            """.formatted(HASH, HASH, url);
        return json.getBytes(StandardCharsets.UTF_8);
    }

    @Test
    void decodesRustPublicationDescriptor() throws IOException {
        ResourcePackDelivery delivery=ResourcePackDescriptorReader.parse(
            sample("https://packs.example.test/" + HASH + ".zip"));
        assertEquals("virgoland", delivery.logicalId());
        assertEquals(1234L, delivery.sizeBytes());
        assertEquals(20, delivery.minecraftSha1Bytes().length);
    }

    @Test
    void rejectsTamperedOrInsecureDescriptors() {
        assertThrows(IOException.class, () ->
            ResourcePackDescriptorReader.parse(
                sample("http://packs.example.test/" + HASH + ".zip")));
        assertThrows(IOException.class, () ->
            ResourcePackDescriptorReader.parse(
                sample("https://packs.example.test/wrong.zip")));
        assertThrows(IOException.class, () ->
            ResourcePackDescriptorReader.parse(new byte[0]));
        assertThrows(IOException.class, () ->
            ResourcePackDescriptorReader.parse(new byte[16385]));
    }
}
