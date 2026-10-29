package io.virgoland.vsmedia.api;

import java.util.HashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;

/**
 * Immutable ordered stack of Minecraft Resource Packs.
 * Lower-priority base layers come first; later layers override earlier assets.
 * Keeping stable base layers separated from frequently updated overlays permits
 * the Minecraft client to reuse previously cached base downloads.
 */
public record ResourcePackStack(List<ResourcePackDelivery> layers) {
    public static final int MAX_LAYERS = 8;

    public ResourcePackStack {
        Objects.requireNonNull(layers, "layers");
        layers = List.copyOf(layers);
        if (layers.isEmpty() || layers.size() > MAX_LAYERS) {
            throw new IllegalArgumentException("Invalid resource pack stack size");
        }
        Set<UUID> ids = new HashSet<>();
        Set<String> names = new HashSet<>();
        for (var layer : layers) {
            if (!ids.add(layer.packId()) || !names.add(layer.logicalId())) {
                throw new IllegalArgumentException("Repeated resource pack UUID or logical ID");
            }
        }
    }
}
