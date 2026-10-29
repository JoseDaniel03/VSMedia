package io.virgoland.vsmedia.api;

import java.util.Optional;
import java.util.concurrent.CompletionStage;

/**
 * Consumer-side API. No dependency on VSRender, Typewriter, VSUiRemake or Bukkit.
 * The future Minecraft bridge will expose an implementation through the platform.
 * CompletionStage methods are asynchronous and must not block Folia region threads.
 */
public interface VSMediaApi {
    int API_VERSION = 1;
    CompletionStage<AssetReceipt> publish(AssetPayload payload);
    CompletionStage<Optional<AssetReceipt>> lookup(AssetId id);
    CompletionStage<byte[]> read(AssetId id);
    CompletionStage<PackReceipt> buildResourcePack();
}
