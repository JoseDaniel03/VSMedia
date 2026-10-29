package io.virgoland.vsmedia.api;

import java.util.HashMap;
import java.util.Iterator;
import java.util.Map;
import java.util.Objects;
import java.util.Optional;
import java.util.UUID;

/**
 * In-memory, bounded, lock-safe resource pack offer lifecycle.
 *
 * Caller must invoke Player.setResourcePack on the Folia entity scheduler;
 * no Minecraft classes, I/O or background tasks exist in this coordinator.
 *
 * Every slot is a (player, pack UUID) pair: distinct packs may be stacked.
 * SHA-1 identifies each offered revision. A changed revision supersedes the
 * preceding one. A player quit must remove all their slots.
 */
public final class ResourcePackSessionCoordinator {
    public static final int MAX_SESSIONS = 4096;

    public enum Decision {
        SEND,
        ALREADY_APPLIED,
        IN_PROGRESS,
        PREVIOUSLY_DECLINED,
        PREVIOUSLY_FAILED,
        AT_CAPACITY
    }

    public enum ClientStatus {
        OFFERED,
        ACCEPTED,
        DOWNLOADED,
        SUCCESSFULLY_LOADED,
        DECLINED,
        FAILED_DOWNLOAD,
        FAILED_RELOAD,
        INVALID_URL,
        DISCARDED,
        FAILED_SEND,
        UNKNOWN;

        public boolean isFailure() {
            return switch (this) {
                case DECLINED, FAILED_DOWNLOAD, FAILED_RELOAD, INVALID_URL, DISCARDED, FAILED_SEND -> true;
                default -> false;
            };
        }
        public static ClientStatus fromPaperStatus(String value) {
            if (value == null) return UNKNOWN;
            try {
                return ClientStatus.valueOf(value);
            } catch (IllegalArgumentException ignored) {
                return UNKNOWN;
            }
        }
    }

    public record Snapshot(String sha1Hex, ClientStatus status) {
        public Snapshot {
            Objects.requireNonNull(sha1Hex);
            Objects.requireNonNull(status);
        }
    }

    private record Slot(UUID player, UUID pack) { }
    private final Map<Slot, Snapshot> sessions = new HashMap<>();

    /**
     * Reserve an offer before issuing a packet so simultaneous callers do not
     * duplicate downloads. On a failed platform call, invoke sendFailed().
     * Failed and declined revisions are not automatically retried.
     */
    public synchronized Decision prepare(
            UUID player, ResourcePackDelivery delivery, boolean retryFailed) {
        Objects.requireNonNull(player, "player");
        Objects.requireNonNull(delivery, "delivery");
        Slot slot = new Slot(player, delivery.packId());
        Snapshot existing = sessions.get(slot);
        if (existing != null && existing.sha1Hex().equals(delivery.sha1Hex())) {
            if (existing.status() == ClientStatus.SUCCESSFULLY_LOADED) {
                return Decision.ALREADY_APPLIED;
            }
            if (existing.status() == ClientStatus.DECLINED && !retryFailed) {
                return Decision.PREVIOUSLY_DECLINED;
            }
            if (existing.status().isFailure() && !retryFailed) {
                return Decision.PREVIOUSLY_FAILED;
            }
            if (!existing.status().isFailure()) {
                return Decision.IN_PROGRESS;
            }
        }
        if (existing == null && sessions.size() >= MAX_SESSIONS) {
            return Decision.AT_CAPACITY;
        }
        sessions.put(slot, new Snapshot(delivery.sha1Hex(), ClientStatus.OFFERED));
        return Decision.SEND;
    }

    /**
     * Client event handler. The status event identifies the pack by UUID,
     * not the SHA-1, so a late event for a replaced version is intrinsically
     * ambiguous on the vanilla protocol. Never regress a terminal status.
     */
    public synchronized boolean onClientStatus(
            UUID player, UUID pack, ClientStatus reported) {
        Objects.requireNonNull(player);
        Objects.requireNonNull(pack);
        Objects.requireNonNull(reported);
        Slot slot = new Slot(player, pack);
        Snapshot previous = sessions.get(slot);
        if (previous == null || reported == ClientStatus.UNKNOWN ||
                reported == ClientStatus.OFFERED ||
                reported == ClientStatus.FAILED_SEND) {
            return false;
        }
        ClientStatus current = previous.status();
        if (current.isFailure() || current == ClientStatus.SUCCESSFULLY_LOADED) {
            return false;
        }
        // Clients may send redundant or out-of-order phase notifications.
        if (reported == ClientStatus.ACCEPTED && current == ClientStatus.DOWNLOADED) {
            return false;
        }
        sessions.put(slot, new Snapshot(previous.sha1Hex(), reported));
        return true;
    }

    /** Reset only the matching revision when the platform send itself fails. */
    public synchronized void sendFailed(
            UUID player, UUID pack, String sha1Hex) {
        Slot slot = new Slot(player, pack);
        Snapshot previous = sessions.get(slot);
        if (previous != null && previous.sha1Hex().equals(sha1Hex) &&
                previous.status() == ClientStatus.OFFERED) {
            sessions.put(slot, new Snapshot(sha1Hex, ClientStatus.FAILED_SEND));
        }
    }

    public synchronized Optional<Snapshot> snapshot(UUID player, UUID pack) {
        return Optional.ofNullable(sessions.get(new Slot(player, pack)));
    }

    /** Call when a player disconnects; their next join can request the pack. */
    public synchronized void onQuit(UUID player) {
        Objects.requireNonNull(player);
        Iterator<Slot> it = sessions.keySet().iterator();
        while (it.hasNext()) {
            if (it.next().player().equals(player)) it.remove();
        }
    }

    /** For reload/unload of the VSMedia plugin. */
    public synchronized void clear() {
        sessions.clear();
    }

    public synchronized int sessionCount() {
        return sessions.size();
    }
}
