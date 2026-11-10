package io.virgoland.vsmedia.plugin;

import io.virgoland.vsmedia.api.ResourcePackDelivery;
import io.virgoland.vsmedia.api.ResourcePackSessionCoordinator;
import io.virgoland.vsmedia.folia.FoliaResourcePackController;
import java.net.URI;
import java.io.IOException;
import java.util.Objects;
import java.util.UUID;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.command.Command;
import org.bukkit.command.CommandSender;
import org.bukkit.command.PluginCommand;
import org.bukkit.entity.Player;
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
public final class VSMediaPlugin extends JavaPlugin {
    private FoliaResourcePackController controller;
    private ResourcePackDelivery active;
    private boolean required;
    private StudioWebServer studioWeb;
    private StudioCoreProcess studioCore;

    @Override
    public void onEnable() {
        saveDefaultConfig();
        loadConfiguredPack();
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

    private void startStudioCore() {
        studioCore=new StudioCoreProcess(getLogger());
        if (getConfig().getBoolean("studio-core.autostart",false)) {
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
            studioWeb=new StudioWebServer(port,getLogger());
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
                if (active == null) {
                    sender.sendMessage(ChatColor.RED
                            + "Configura primero un pack HTTPS valido en config.yml.");
                    return true;
                }
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
                // offerTo schedules on the player's own Folia entity scheduler.
                controller.offerTo(target, true);
                sender.sendMessage(ChatColor.GREEN + "Oferta programada para " + target.getName());
            }
            default -> sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia <status|web|reload|send [jugador]>");
        }
        return true;
    }

    @Override
    public void onDisable() {
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
