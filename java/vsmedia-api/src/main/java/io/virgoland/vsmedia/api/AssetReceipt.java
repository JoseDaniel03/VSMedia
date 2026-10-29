package io.virgoland.vsmedia.api;
public record AssetReceipt(AssetId id, AssetType type, String sha256, long size, long revision) {}
