package io.virgoland.vsmedia.api;

/** Build metadata only; the distribution layer is a separate service. */
public record PackReceipt(String sha256, long size, long revision) {}
