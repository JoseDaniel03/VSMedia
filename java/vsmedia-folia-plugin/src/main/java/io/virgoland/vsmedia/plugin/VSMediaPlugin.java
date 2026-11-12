package io.virgoland.vsmedia.plugin;

import io.virgoland.vsmedia.api.ResourcePackDelivery;
import io.virgoland.vsmedia.api.ResourcePackSessionCoordinator;
import io.virgoland.vsmedia.folia.FoliaResourcePackController;
import java.net.URI;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Map;
import java.util.Locale;
import java.util.concurrent.ConcurrentHashMap;
import java.util.regex.Pattern;
import java.io.IOException;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.Plugin;
import org.bukkit.event.HandlerList;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Test-only Folia/Paper entrypoint.
 *
 * The original FullCanvas Studio is served from this plugin JAR without UI changes.
 * The entity scheduler inside FoliaResourcePackController is the only path
 * used to offer packs to players.
 */
@SuppressWarnings("deprecation")
public final class VSMediaPlugin extends JavaPlugin implements Listener {
    private static final Pattern PRODUCER = Pattern.compile("[a-z0-9_-]{1,32}");
    private final Object packLock = new Object();
    private final Map<UUID, Map<String, List<ResourcePackDelivery>>> external = new ConcurrentHashMap<>();
    private volatile FoliaResourcePackController controller;
    private volatile ResourcePackDelivery active;
    private boolean required;
    private StudioWebServer studioWeb;
    private StudioCoreProcess studioCore;
    private PlaybackControl playback;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        loadConfiguredPack();
        getServer().getPluginManager().registerEvents(this, this);
        playback = new PlaybackControl(this);
        getServer().getPluginManager().registerEvents(playback, this);
        startStudioCore();
        startStudioWeb();
        PluginCommand command = Objects.requireNonNull(getCommand("vsmedia"),
                "The VSMedia plugin.yml does not declare /vsmedia");
        command.setExecutor(this::command);
        getLogger().info("VSMedia 26.3 enabled: web autostart; /vsmedia web muestra la dirección");
        getLogger().info(active == null
                ? "No resource pack is configured. Studio remains available from this plugin."
                : "Ready to offer pack '" + active.logicalId() + "' to joining players.");
    }

    /**
     * Cooperative resource-pack handoff for any installed Bukkit/Folia plugin.
     * Receives public, already hosted HTTPS descriptors; no image/ZIP processing
     * is performed by Minecraft. Works from async producer callbacks.
     *
     * Each descriptor: uuid, url, sha1 (optional sizeBytes, defaults to 0).
     * Returns true ONLY after atomic validation and scheduling of one offer.
     * Consumers must retain their own send path when this returns false.
     */
    public boolean offerPluginResourcePacks(
            Plugin producer, UUID playerId, List<Map<String, String>> descriptors) {
        if (!getConfig().getBoolean("external-packs.enabled", true) ||
                producer == null || playerId == null || descriptors == null ||
                !producer.isEnabled() ||
                getServer().getPluginManager().getPlugin(producer.getName()) != producer) {
            return false;
        }
        String key = producer.getName().toLowerCase(Locale.ROOT);
        if (!PRODUCER.matcher(key).matches() || descriptors.size() > 8) return false;
        List<ResourcePackDelivery> received = new ArrayList<>(descriptors.size());
        try {
            for (Map<String, String> descriptor : descriptors) {
                if (descriptor == null) return false;
                UUID uuid = UUID.fromString(descriptor.get("uuid"));
                String id = key + "_" + uuid.toString().replace("-", "").substring(0, 20);
                String url = descriptor.get("url");
                String sha1 = descriptor.get("sha1");
                if (url == null || url.length() > 2048 || sha1 == null) return false;
                String sizeText = descriptor.getOrDefault("sizeBytes", "0");
                long size = Long.parseLong(sizeText);
                received.add(new ResourcePackDelivery(id, uuid, URI.create(url),
                        sha1.toLowerCase(Locale.ROOT), size));
            }
            if (received.stream().map(ResourcePackDelivery::packId).distinct().count()
                    != received.size()) return false;
        } catch (RuntimeException invalid) {
            getLogger().warning("Invalid external Resource Pack from " + key + ": "
                    + invalid.getClass().getSimpleName());
            return false;
        }
        List<ResourcePackDelivery> combined;
        synchronized (packLock) {
            Map<String, List<ResourcePackDelivery>> old =
                    external.getOrDefault(playerId, Map.of());
            Map<String, List<ResourcePackDelivery>> revised = new LinkedHashMap<>(old);
            if (received.isEmpty()) revised.remove(key);
            else revised.put(key, List.copyOf(received));
            combined = orderedLayers(revised);
            if (combined == null) return false;
            if (revised.isEmpty()) external.remove(playerId);
            else external.put(playerId, Map.copyOf(revised));
        }
        return scheduleOffer(playerId, combined);
    }

    /** Called by the provider when it disables or stops managing packs. */
    public void withdrawPluginResourcePacks(Plugin producer) {
        if (producer == null) return;
        String key = producer.getName().toLowerCase(Locale.ROOT);
        Map<UUID, List<ResourcePackDelivery>> changed = new HashMap<>();
        synchronized (packLock) {
            for (var entry : List.copyOf(external.entrySet())) {
                if (!entry.getValue().containsKey(key)) continue;
                Map<String, List<ResourcePackDelivery>> revised =
                        new LinkedHashMap<>(entry.getValue());
                revised.remove(key);
                List<ResourcePackDelivery> layers = orderedLayers(revised);
                if (revised.isEmpty()) external.remove(entry.getKey());
                else external.put(entry.getKey(), Map.copyOf(revised));
                if (layers != null) changed.put(entry.getKey(), layers);
            }
        }
        changed.forEach(this::scheduleOffer);
    }

    private List<ResourcePackDelivery> orderedLayers(
            Map<String, List<ResourcePackDelivery>> sources) {
        List<ResourcePackDelivery> layers = new ArrayList<>();
        if (active != null) layers.add(active);
        sources.entrySet().stream().sorted(Map.Entry.comparingByKey())
                .forEach(e -> layers.addAll(e.getValue()));
        if (layers.size() > 8 || layers.stream().map(ResourcePackDelivery::packId)
                .distinct().count() != layers.size()) return null;
        return List.copyOf(layers);
    }

    /**
     * Global-region lookup is the only Bukkit operation used when a producer
     * offers packs asynchronously. Pack packets run on the entity scheduler.
     */
    private boolean scheduleOffer(UUID playerId, List<ResourcePackDelivery> layers) {
        try {
            getServer().getGlobalRegionScheduler().run(this, task -> {
                Player player = Bukkit.getPlayer(playerId);
                FoliaResourcePackController current = controller;
                if (player != null && player.isOnline() && current != null) {
                    current.offerLayersTo(player, false, layers);
                }
            });
            return true;
        } catch (IllegalStateException stopped) {
            return false;
        }
    }

    @EventHandler
    public void onPlayerQuit(PlayerQuitEvent event) {
        external.remove(event.getPlayer().getUniqueId());
    }

    private void startStudioCore() {
        studioCore=new StudioCoreProcess(getLogger());
        if (getConfig().getBoolean("studio-core.autostart",true)) {
            studioCore.start(getDataFolder().toPath());
        } else {
            getLogger().info("Auto-start of bundled VSCore is disabled.");
        }
    }
    private void stopStudioCore() {
        if (studioCore != null) {
            studioCore.close();
            studioCore=null;
        }
    }
    private void startStudioWeb() {
        if (!getConfig().getBoolean("studio-web.enabled", true)) {
            getLogger().info("Home/Audio Studio web is disabled in config.yml");
            return;
        }
        int port=getConfig().getInt("studio-web.port", 3060);
        try {
            studioWeb=new StudioWebServer(port,getLogger(), args -> 
                    getServer().getGlobalRegionScheduler().run(this, task ->
                            playback.execute(getServer().getConsoleSender(), args[0], args)));
            studioWeb.start();
        } catch (IOException | RuntimeException error) {
            studioWeb=null;
            getLogger().severe("Unable to open the packaged Studio web: "
                    + error.getMessage());
        }
    }
    private void stopStudioWeb() {
        if (studioWeb != null) {
            studioWeb.close();
            studioWeb=null;
        }
    }
    private void loadConfiguredPack() {
        if (controller != null) {
            HandlerList.unregisterAll(controller);
            controller.close();
            controller = null;
        }
        active = null;
        required = getConfig().getBoolean("resource-pack.required", false);
        ResourcePackSessionCoordinator sessions = new ResourcePackSessionCoordinator();
        controller = new FoliaResourcePackController(this, sessions, required);
        if (getConfig().getBoolean("resource-pack.enabled", false)) {
            try {
                ResourcePackDelivery prepared = new ResourcePackDelivery(
                        getConfig().getString("resource-pack.logical-id", "virgoland"),
                        UUID.fromString(Objects.requireNonNull(
                                getConfig().getString("resource-pack.uuid"), "missing UUID")),
                        URI.create(Objects.requireNonNull(
                                getConfig().getString("resource-pack.url"), "missing URL")),
                        Objects.requireNonNull(
                                getConfig().getString("resource-pack.sha1"), "missing SHA1"),
                        getConfig().getLong("resource-pack.size-bytes", -1));
                controller.setCurrent(prepared);
                active = prepared;
            } catch (RuntimeException invalid) {
                getLogger().severe("Resource pack config invalid; offers disabled: "
                        + invalid.getMessage());
            }
        }
        getServer().getPluginManager().registerEvents(controller, this);
    }

    private boolean command(CommandSender sender, Command command,
                            String alias, String[] args) {
        if (!sender.hasPermission("vsmedia.admin")) {
            sender.sendMessage(ChatColor.RED + "No tenes permiso para administrar VSMedia.");
            return true;
        }
        String sub = args.length == 0 ? "status" : args[0].toLowerCase(java.util.Locale.ROOT);
        switch (sub) {
            case "status" -> {
                sender.sendMessage(ChatColor.GOLD + "[VSMedia] 26.3 TEST"
                        + ChatColor.GRAY + " | Folia adapter cargado");
                sender.sendMessage(ChatColor.GRAY + "Pack configurado: "
                        + (active == null ? "NO" : active.logicalId()));
                sender.sendMessage(ChatColor.GRAY + "Fuentes externas activas: "
                        + external.values().stream().mapToInt(Map::size).sum());
                sender.sendMessage(ChatColor.GRAY + "Ofertas rastreadas: "
                        + controller.coordinator().sessionCount());
                sender.sendMessage(ChatColor.GOLD + "VSMedia Web: "
                        + (studioWeb == null ? "OFFLINE" : studioWeb.url()));
                sender.sendMessage(ChatColor.YELLOW + "VSCore audio Rust: "
                        + (studioCore != null && studioCore.isRunning()
                            ? "activo" : "inactivo (revisar vsmedia-core.log)"));
            }
            case "web" -> {
                sender.sendMessage(ChatColor.GOLD + "VSMedia Studio: "
                        + (studioWeb == null ? "OFFLINE" : studioWeb.url()));
                sender.sendMessage(ChatColor.GRAY
                        + "Acceso remoto: tunel SSH recomendado, no abrir el puerto publicamente.");
            }
            case "reload" -> {
                reloadConfig();
                loadConfiguredPack();
                stopStudioWeb();
                stopStudioCore();
                startStudioCore();
                startStudioWeb();
                sender.sendMessage(ChatColor.GREEN + "Configuracion VSMedia recargada. Pack: "
                        + (active == null ? "desactivado/invalido" : active.logicalId()));
            }
            case "send" -> {
                Player target;
                if (args.length >= 2) {
                    target = Bukkit.getPlayerExact(args[1]);
                } else if (sender instanceof Player player) {
                    target = player;
                } else {
                    sender.sendMessage(ChatColor.YELLOW + "Uso: /vsmedia send <jugador>");
                    return true;
                }
                if (target == null) {
                    sender.sendMessage(ChatColor.RED + "Jugador no encontrado.");
                    return true;
                }
                // offerLayersTo schedules all packet operations on the owning entity.
                Map<String, List<ResourcePackDelivery>> sources =
                        external.getOrDefault(target.getUniqueId(), Map.of());
                List<ResourcePackDelivery> layers = orderedLayers(sources);
                if (layers == null || layers.isEmpty()) {
                    sender.sendMessage(ChatColor.RED + "No hay packs registrados para este jugador.");
                    return true;
                }
                controller.offerLayersTo(target, true, layers);
                sender.sendMessage(ChatColor.GREEN + "Oferta programada para " + target.getName());
            }
            case "title", "subtitle", "preview", "chatfont", "play", "sound", "stopsound" ->
                    playback.execute(sender, sub, args);
            default -> sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia <status|web|reload|send|title|subtitle|preview|chatfont|play|stopsound>");
        }
        return true;
    }

    @Override
    public void onDisable() {
        external.clear();
        HandlerList.unregisterAll((Listener) this);
        if (playback != null) HandlerList.unregisterAll(playback);
        stopStudioWeb();
        stopStudioCore();
        if (controller != null) {
            HandlerList.unregisterAll(controller);
            controller.close();
            controller = null;
        }
        active = null;
    }
}
