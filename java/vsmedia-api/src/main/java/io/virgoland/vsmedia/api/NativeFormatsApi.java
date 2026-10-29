package io.virgoland.vsmedia.api;

import java.util.concurrent.CompletionStage;

/**
 * Additive API, separate from VSMediaApi to preserve existing implementers.
 * Manifests are JSON UTF-8, validated and versioned by the Rust engine.
 */
public interface NativeFormatsApi {
    CompletionStage<AssetReceipt> publishNative(
            AssetId id, NativeFormat format, byte[] manifestJson);
    CompletionStage<byte[]> readNative(AssetId id, NativeFormat format);
}
