package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import io.virgoland.vsmedia.api.ResourcePackDelivery;
import java.io.IOException;
import java.net.URI;
import java.util.Objects;
import java.util.UUID;

/**
 * Decode the exact immutable descriptor printed by Rust pack-stage.
 * Input must come from trusted VSMedia administration, never a player request.
 */
public final class ResourcePackDescriptorReader {
    private static final ObjectMapper MAPPER = new ObjectMapper();
    private static final int MAX_BYTES = 16 * 1024;

    private ResourcePackDescriptorReader() {}

    public static ResourcePackDelivery parse(byte[] json) throws IOException {
        Objects.requireNonNull(json);
        if (json.length == 0 || json.length > MAX_BYTES) {
            throw new IOException("Resource pack descriptor exceeds input limit");
        }
        try {
            JsonNode data = MAPPER.readTree(json);
            if (data == null || !data.isObject()) {
                throw new IOException("Resource pack descriptor must be an object");
            }
            String logicalId = requiredText(data, "logical_id");
            UUID uuid = UUID.fromString(requiredText(data, "uuid"));
            String sha1 = requiredText(data, "sha1");
            String sha256 = requiredText(data, "sha256");
            String filename = requiredText(data, "filename");
            String url = requiredText(data, "url");
            JsonNode size = data.path("size_bytes");
            if (!size.isIntegralNumber() || !size.canConvertToLong()) {
                throw new IOException("Invalid resource pack size");
            }
            if (!sha256.matches("[0-9a-f]{64}") || !filename.equals(sha256 + ".zip")) {
                throw new IOException("Descriptor filename does not match SHA-256");
            }
            URI uri = URI.create(url);
            if (!uri.getPath().endsWith("/" + filename)) {
                throw new IOException("Descriptor URL does not name the published ZIP");
            }
            return new ResourcePackDelivery(logicalId, uuid, uri, sha1, size.longValue());
        } catch (IllegalArgumentException ex) {
            throw new IOException("Invalid resource pack descriptor", ex);
        }
    }

    private static String requiredText(JsonNode object, String key) throws IOException {
        JsonNode value = object.path(key);
        if (!value.isTextual() || value.asText().isEmpty()) {
            throw new IOException("Missing resource pack descriptor field: " + key);
        }
        return value.asText();
    }
}
