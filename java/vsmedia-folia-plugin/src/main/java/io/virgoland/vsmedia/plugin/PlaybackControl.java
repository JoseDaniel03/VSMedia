package io.virgoland.vsmedia.plugin;

import io.papermc.paper.event.player.AsyncChatEvent;
import java.time.Duration;
import java.util.ArrayList;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.UUID;
import java.util.concurrent.ConcurrentHashMap;
import net.kyori.adventure.key.Key;
import net.kyori.adventure.sound.Sound;
import net.kyori.adventure.sound.SoundStop;
import net.kyori.adventure.text.Component;
import net.kyori.adventure.text.format.NamedTextColor;
import net.kyori.adventure.text.format.TextColor;
import net.kyori.adventure.title.Title;
import org.bukkit.Bukkit;
import org.bukkit.ChatColor;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;
import org.bukkit.event.EventHandler;
import org.bukkit.event.Listener;
import org.bukkit.event.player.PlayerQuitEvent;
import org.bukkit.plugin.java.JavaPlugin;

/**
 * Applies already-published resource-pack fonts and sounds to Minecraft players.
 * This class does NOT convert, distribute or claim to publish assets.
 *
 * Commands originate on arbitrary Folia regions. All player packet operations
 * run through that player's entity scheduler, not the caller's region.
 */
final class PlaybackControl implements Listener {
    private final JavaPlugin plugin;
    private final Map<UUID, Key> chatFonts = new ConcurrentHashMap<>();

    PlaybackControl(JavaPlugin plugin) {
        this.plugin = plugin;
    }

    void execute(CommandSender sender, String action, String[] args) {
        switch (action) {
            case "title", "subtitle", "preview" -> display(sender, action, args);
            case "play", "sound" -> sound(sender, args);
            case "stopsound" -> stopSound(sender, args);
            case "chatfont" -> chatfont(sender, args);
            default -> throw new IllegalArgumentException("Unknown VSMedia playback action");
        }
    }

    private void display(CommandSender sender, String action, String[] args) {
        if (args.length < 5) {
            sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia " + action + " <jugador|*> <fuente> <color> <texto...>");
            return;
        }
        List<Player> targets = targets(sender, args[1]);
        if (targets.isEmpty()) return;
        Key font = font(sender, args[2]);
        TextColor color = color(sender, args[3]);
        if (font == null || color == null) return;
        String message = String.join(" ", java.util.Arrays.copyOfRange(args, 4, args.length));
        if (message.length() > 512) {
            sender.sendMessage(ChatColor.RED + "El texto no puede superar 512 caracteres.");
            return;
        }
        Component text = Component.text(message).font(font).color(color);
        Component secondary = Component.text("Vista previa: " + font.asString())
                .font(font).color(NamedTextColor.GRAY);
        Title.Times times = Title.Times.times(
                Duration.ofMillis(400), Duration.ofSeconds(4), Duration.ofMillis(600));
        for (Player target : targets) {
            target.getScheduler().run(plugin, task -> {
                if (!target.isOnline()) return;
                Title title = switch (action) {
                    case "subtitle" -> Title.title(Component.empty(), text, times);
                    case "preview" -> Title.title(text, secondary, times);
                    default -> Title.title(text, Component.empty(), times);
                };
                target.showTitle(title);
            }, () -> {});
        }
        sender.sendMessage(ChatColor.GREEN + "Enviado a " + targets.size()
                + " jugador(es). La fuente debe estar en un pack aceptado.");
    }

    private void sound(CommandSender sender, String[] args) {
        if (args.length < 3 || args.length > 5) {
            sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia play <jugador|*> <sonido_namespace:id> [volumen] [tono]");
            return;
        }
        List<Player> targets = targets(sender, args[1]);
        if (targets.isEmpty()) return;
        Key key = font(sender, args[2]);
        if (key == null) return;
        float volume = number(sender, args.length >= 4 ? args[3] : "1", 0.0f, 4f, "volumen");
        float pitch = number(sender, args.length >= 5 ? args[4] : "1", 0.5f, 2f, "tono");
        if (Float.isNaN(volume) || Float.isNaN(pitch)) return;
        Sound sound = Sound.sound(key, Sound.Source.MASTER, volume, pitch);
        for (Player target : targets) {
            target.getScheduler().run(plugin, task -> {
                if (target.isOnline()) target.playSound(sound);
            }, () -> {});
        }
        sender.sendMessage(ChatColor.GREEN + "Reproduccion programada para "
                + targets.size() + " jugador(es): " + key.asString()
                + ". El archivo y sounds.json deben estar publicados en el pack.");
    }

    private void stopSound(CommandSender sender, String[] args) {
        if (args.length != 3) {
            sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia stopsound <jugador|*> <sonido_namespace:id>");
            return;
        }
        List<Player> targets = targets(sender, args[1]);
        if (targets.isEmpty()) return;
        Key key = font(sender, args[2]);
        if (key == null) return;
        SoundStop stop = SoundStop.named(key);
        for (Player target : targets) {
            target.getScheduler().run(plugin, task -> {
                if (target.isOnline()) target.stopSound(stop);
            }, () -> {});
        }
        sender.sendMessage(ChatColor.GREEN + "Detencion programada para " + targets.size()
                + " jugador(es): " + key.asString());
    }

    private void chatfont(CommandSender sender, String[] args) {
        if (args.length != 3) {
            sender.sendMessage(ChatColor.YELLOW
                    + "Uso: /vsmedia chatfont <jugador> <fuente|default>");
            return;
        }
        Player target = Bukkit.getPlayerExact(args[1]);
        if (target == null) {
            sender.sendMessage(ChatColor.RED + "Jugador no encontrado.");
            return;
        }
        if ("default".equalsIgnoreCase(args[2])) {
            chatFonts.remove(target.getUniqueId());
        } else {
            Key font = font(sender, args[2]);
            if (font == null) return;
            chatFonts.put(target.getUniqueId(), font);
        }
        sender.sendMessage(ChatColor.GREEN + "Fuente del texto del chat de "
                + target.getName() + ": " + args[2]
                + ". No cambia el cuadro de escritura ni otros mensajes del cliente.");
    }

    /**
     * Only styles the player's outgoing chat text, preserving Paper's own
     * rendering, viewers, signatures and moderation policies.
     */
    @EventHandler
    public void onAsyncChat(AsyncChatEvent event) {
        Key key = chatFonts.get(event.getPlayer().getUniqueId());
        if (key != null && !event.isCancelled()) {
            event.message(event.message().font(key));
        }
    }

    @EventHandler
    public void onQuit(PlayerQuitEvent event) {
        chatFonts.remove(event.getPlayer().getUniqueId());
    }

    private List<Player> targets(CommandSender sender, String input) {
        if ("*".equals(input)) {
            return new ArrayList<>(Bukkit.getOnlinePlayers());
        }
        Player player = Bukkit.getPlayerExact(input);
        if (player == null) {
            sender.sendMessage(ChatColor.RED + "Jugador no encontrado: " + input);
            return List.of();
        }
        return List.of(player);
    }

    private static Key font(CommandSender sender, String input) {
        try {
            // The registry is in the pack, not in the Bukkit server.
            return Key.key(input.toLowerCase(Locale.ROOT));
        } catch (IllegalArgumentException invalid) {
            sender.sendMessage(ChatColor.RED + "ID invalido. Ejemplo: vsmedia:imported/archivo");
            return null;
        }
    }

    private static TextColor color(CommandSender sender, String input) {
        TextColor color = input.startsWith("#")
                ? TextColor.fromHexString(input) : NamedTextColor.NAMES.value(input.toLowerCase(Locale.ROOT));
        if (color == null) {
            sender.sendMessage(ChatColor.RED + "Color invalido. Usa #RRGGBB, gold, white, red...");
        }
        return color;
    }

    private static float number(CommandSender sender, String text, float min, float max, String label) {
        try {
            float result = Float.parseFloat(text);
            if (Float.isFinite(result) && result >= min && result <= max) return result;
        } catch (NumberFormatException ignored) {}
        sender.sendMessage(ChatColor.RED + "Valor de " + label + " invalido ("
                + min + " a " + max + ").");
        return Float.NaN;
    }
}
