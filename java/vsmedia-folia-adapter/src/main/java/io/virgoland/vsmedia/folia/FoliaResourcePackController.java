package io.virgoland.vsmedia.folia;

import io.virgoland.vsmedia.api.ResourcePackDelivery;
import io.virgoland.vsmedia.api.ResourcePackSessionCoordinator;
import io.virgoland.vsmedia.api.ResourcePackSessionCoordinator.ClientStatus;
import io.virgoland.vsmedia.api.ResourcePackSessionCoordinator.Decision;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerJoinEvent;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.event.player.PlayerResourcePackStatusEvent;
import org.bukkit.plugin.Plugin;
import java.util.Objects;
import java.util.UUID;

/**
 * Source-only Folia-safe listener. The owning plugin registers this adapter.
 * No ZIP, hashing, HTTP or blocking network calls occur on entity threads.
 */
public final class FoliaResourcePackController implements Listener {
    private final Plugin plugin;
    private final ResourcePackSessionCoordinator sessions;
    private final boolean required;
    private volatile ResourcePackDelivery current;

    public FoliaResourcePackController(
            Plugin plugin, ResourcePackSessionCoordinator sessions, boolean required) {
        this.plugin = Objects.requireNonNull(plugin, "plugin");
        this.sessions = Objects.requireNonNull(sessions, "sessions");
        this.required = required;
    }

    public void setCurrent(ResourcePackDelivery delivery) {
        this.current = Objects.requireNonNull(delivery, "delivery");
    }

    /** Schedule a pack offer on the player entity's owning Folia region. */
    public void offerTo(Player player, boolean retryFailed) {
        Objects.requireNonNull(player, "player");
        UUID playerId = player.getUniqueId();
        player.getScheduler().run(plugin, task -> {
            if (!player.isOnline()) {
                sessions.onQuit(playerId);
                return;
            }
            ResourcePackDelivery delivery = current;
            if (delivery == null) return;
            Decision decision = sessions.prepare(playerId, delivery, retryFailed);
            if (decision != Decision.SEND) return;
            try {
                player.setResourcePack(
                    delivery.packId(),
                    delivery.downloadUrl().toASCIIString(),
                    delivery.minecraftSha1Bytes(),
                    (net.kyori.adventure.text.Component) null,
                    required
                );
            } catch (RuntimeException error) {
                sessions.sendFailed(playerId, delivery.packId(), delivery.sha1Hex());
                plugin.getLogger().warning("VSMedia pack offer failed: "
                    + error.getClass().getSimpleName());
            }
        }, () -> sessions.onQuit(playerId));
    }

    @EventHandler
    public void onJoin(PlayerJoinEvent event) {
        offerTo(event.getPlayer(), false);
    }

    @EventHandler
    public void onStatus(PlayerResourcePackStatusEvent event) {
        sessions.onClientStatus(
            event.getPlayer().getUniqueId(),
            event.getID(),
            ClientStatus.fromPaperStatus(event.getStatus().name())
        );
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        sessions.onQuit(event.getPlayer().getUniqueId());
    }

    public void close() {
        current = null;
        sessions.clear();
    }

    public ResourcePackSessionCoordinator coordinator() {
        return sessions;
    }
}
