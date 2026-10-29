package io.virgoland.vsmedia.api;

import java.util.Objects;

/** Logical resource identity; never use this record as a filesystem path. */
public record AssetId(String namespace, String name) {
    public AssetId {
        Objects.requireNonNull(namespace);
        Objects.requireNonNull(name);
        if (!namespace.matches("[a-z0-9_-]{1,64}")
                || name.length() > 256
                || !name.matches("[a-z0-9_.-/]+")
                || name.startsWith("/") || name.endsWith("/")
                || name.contains("//") || name.equals(".") || name.equals("..")
                || name.startsWith("../") || name.contains("/../")
                || name.startsWith("./") || name.contains("/./")
                || name.endsWith("/.") || name.endsWith("/..")) {
            throw new IllegalArgumentException("Invalid media resource id");
        }
    }
    public String qualified() { return namespace + ":" + name; }
}
