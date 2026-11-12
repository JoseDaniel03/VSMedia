package io.virgoland.vsmedia.plugin;

import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import io.virgoland.vsmedia.api.ResourcePackDelivery;
import io.virgoland.vsmedia.api.ResourcePackStack;
import java.io.IOException;
import java.io.InputStream;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashSet;
import java.util.List;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.function.Consumer;
import java.util.logging.Logger;

/**
 * HTTPS publication manifest watcher. Only a bounded JSON descriptor is fetched;
 * players download immutable ZIPs directly from the public CDN.
 *
 * Never touches Bukkit API or blocking I/O on a Folia region thread.
 * On HTTP or validation errors, keeps the previous known-good publication.
 */
final class RemotePackCatalog implements AutoCloseable {
    private static final int MAX_MANIFEST = 256 * 1024;
    private final URI manifestUrl;
    private final HttpClient client;
    private final Logger logger;
    private final Consumer<List<ResourcePackDelivery>> onChange;
    private final ScheduledExecutorService poller;
    private final long intervalSeconds;
    private volatile List<ResourcePackDelivery> current = List.of();

    RemotePackCatalog(URI manifestUrl, long intervalSeconds, Logger logger,
                      Consumer<List<ResourcePackDelivery>> onChange) {
        this.manifestUrl = validateManifestUrl(manifestUrl);
        this.logger = Objects.requireNonNull(logger);
        this.onChange = Objects.requireNonNull(onChange);
        this.intervalSeconds = Math.max(60L, Math.min(intervalSeconds, 3600L));
        this.client = HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(5))
                .followRedirects(HttpClient.Redirect.NEVER)
                .build();
        this.poller = Executors.newSingleThreadScheduledExecutor(task -> {
            Thread thread = new Thread(task, "vsmedia-pack-manifest");
            thread.setDaemon(true);
            return thread;
        });
    }

    static URI validateManifestUrl(URI uri) {
        Objects.requireNonNull(uri);
        if (!"https".equalsIgnoreCase(uri.getScheme())
                || uri.getHost() == null
                || uri.getRawUserInfo() != null
                || uri.getRawQuery() != null
                || uri.getRawFragment() != null) {
            throw new IllegalArgumentException("Manifest debe usar HTTPS sin credenciales ni query");
        }
        return uri;
    }

    void start() {
        poller.scheduleWithFixedDelay(this::safeRefresh, 0L, intervalSeconds, TimeUnit.SECONDS);
    }

    List<ResourcePackDelivery> current() {
        return current;
    }

    private void safeRefresh() {
        try {
            refresh();
        } catch (Exception error) {
            logger.warning("VSPack: no se pudo consultar la publicación HTTPS; se conserva la anterior: "
                    + error.getClass().getSimpleName() + ": " + error.getMessage());
        }
    }

    void refresh() throws IOException, InterruptedException {
        HttpRequest request = HttpRequest.newBuilder(manifestUrl)
                .GET().timeout(Duration.ofSeconds(10))
                .header("Accept", "application/json")
                .build();
        HttpResponse<InputStream> response = client.send(request, HttpResponse.BodyHandlers.ofInputStream());
        try (InputStream stream = response.body()) {
            if (response.statusCode() != 200) {
                throw new IOException("HTTP " + response.statusCode());
            }
            byte[] payload = stream.readNBytes(MAX_MANIFEST + 1);
            if (payload.length > MAX_MANIFEST) {
                throw new IOException("Manifest remoto supera 256 KiB");
            }
            List<ResourcePackDelivery> next = parseManifest(
                    new String(payload, StandardCharsets.UTF_8), manifestUrl);
            if (!next.equals(current)) {
                onChange.accept(next);
                current = next;
                logger.info("VSPack actualizado desde CDN: " + next.size() + " capas");
            }
        }
    }

    /** Validate everything BEFORE accepting a new pack stack. */
    static List<ResourcePackDelivery> parseManifest(String json, URI source) {
        validateManifestUrl(source);
        JsonObject root = JsonParser.parseString(json).getAsJsonObject();
        if (!"vspack-publication-v1".equals(root.get("schema").getAsString())
                || !"uploaded_to_r2".equals(root.get("status").getAsString())) {
            throw new IllegalArgumentException("La versión remota no está publicada");
        }
        JsonArray items = root.getAsJsonArray("packs");
        if (items == null || items.isEmpty() || items.size() >= ResourcePackStack.MAX_LAYERS) {
            throw new IllegalArgumentException("Cantidad de capas fuera de límite");
        }
        List<ResourcePackDelivery> layers = new ArrayList<>();
        Set<UUID> ids = new HashSet<>();
        Set<String> names = new HashSet<>();
        for (JsonElement item : items) {
            JsonObject pack = item.getAsJsonObject();
            String logical = pack.get("logical_id").getAsString();
            UUID uuid = UUID.fromString(pack.get("uuid").getAsString());
            URI uri = URI.create(pack.get("url").getAsString());
            if (!source.getHost().equalsIgnoreCase(uri.getHost())) {
                throw new IllegalArgumentException("La URL de pack debe usar el mismo CDN del manifest");
            }
            String hash = pack.get("sha1").getAsString();
            long bytes = pack.get("bytes").getAsLong();
            int priority = pack.get("priority").getAsInt();
            if (priority != layers.size()) {
                throw new IllegalArgumentException("Orden de capas inválido");
            }
            ResourcePackDelivery delivery = new ResourcePackDelivery(
                    logical, uuid, uri, hash, bytes);
            if (!ids.add(uuid) || !names.add(logical)) {
                throw new IllegalArgumentException("UUID o nombre repetido en manifest");
            }
            layers.add(delivery);
        }
        return List.copyOf(layers);
    }

    @Override
    public void close() {
        poller.shutdownNow();
    }
}
