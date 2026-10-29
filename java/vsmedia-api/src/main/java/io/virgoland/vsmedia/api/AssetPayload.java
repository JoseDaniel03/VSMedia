package io.virgoland.vsmedia.api;

import java.util.Objects;

/** Defensive copy: clients cannot mutate the payload after submission. */
public record AssetPayload(AssetId id, AssetType type, byte[] data) {
    public AssetPayload {
        Objects.requireNonNull(id);
        Objects.requireNonNull(type);
        data = Objects.requireNonNull(data).clone();
    }
    @Override public byte[] data() { return data.clone(); }
}
