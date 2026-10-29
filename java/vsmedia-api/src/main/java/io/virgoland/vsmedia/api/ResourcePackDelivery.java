package io.virgoland.vsmedia.api;

import java.net.URI;
import java.util.HexFormat;
import java.util.Objects;
import java.util.UUID;

/**
 * Immutable Minecraft resource pack offer. Hash is SHA-1 (20 bytes), used
 * by the native client cache. The VSMedia storage checksum remains SHA-256.
 * No direct Bukkit/Folia dependency is introduced in the shared API.
 */
public record ResourcePackDelivery(
        String logicalId, UUID packId, URI downloadUrl, String sha1Hex, long sizeBytes) {
    public ResourcePackDelivery {
        Objects.requireNonNull(packId, "packId");
        Objects.requireNonNull(downloadUrl, "downloadUrl");
        Objects.requireNonNull(sha1Hex, "sha1Hex");
        if (logicalId == null || !logicalId.matches("[a-z0-9_-]{1,64}")) {
            throw new IllegalArgumentException("Invalid logical pack ID");
        }
        if (!"https".equalsIgnoreCase(downloadUrl.getScheme())
                || downloadUrl.getHost() == null
                || downloadUrl.getRawUserInfo() != null
                || downloadUrl.getRawQuery() != null
                || downloadUrl.getRawFragment() != null) {
            throw new IllegalArgumentException("Resource pack URL must be HTTPS");
        }
        if (sha1Hex.length() != 40
                || !sha1Hex.matches("[0-9a-f]{40}")
                || sizeBytes < 0 || sizeBytes > 64L * 1024 * 1024) {
            throw new IllegalArgumentException("Invalid Minecraft pack hash or size");
        }
    }
    public byte[] minecraftSha1Bytes() {
        return HexFormat.of().parseHex(sha1Hex);
    }
}
