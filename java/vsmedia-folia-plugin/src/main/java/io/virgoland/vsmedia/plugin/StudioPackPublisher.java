package io.virgoland.vsmedia.plugin;

import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import io.virgoland.vsmedia.api.ResourcePackDelivery;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.security.MessageDigest;
import java.time.Duration;
import java.util.HexFormat;
import java.util.UUID;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.function.Consumer;
import java.util.logging.Logger;
import java.util.zip.ZipFile;

/**
 * Builds the current Resource Pack from VSCore, keeps one immutable local
 * snapshot, and serves only that ZIP to Minecraft clients.
 *
 * Minecraft requires a URL + SHA-1 for a resource pack. The Studio web and
 * VSCore stay loopback-only; this tiny server is independently configurable
 * because players must be able to reach the ZIP from their own machines.
 */
public final class StudioPackPublisher implements AutoCloseable {
    private static final long MAX_ZIP = 128L * 1024 * 1024;
    private final Logger logger;
    private final Path dataDir;
    private final URI publicUrl;
    private final URI backendExportUrl;
    private final UUID packId;
    private final String logicalId;
    private final boolean enabled;
    private final HttpClient client;
    private final ExecutorService worker;
    private final ExecutorService httpExecutor;
    private final Consumer<ResourcePackDelivery> onPublished;
    private volatile HttpServer server;
    private volatile ResourcePackDelivery current;

    public StudioPackPublisher(
            Logger logger,
            Path dataDir,
            String bindAddress,
            int port,
            URI publicUrl,
            UUID packId,
            String logicalId,
            boolean enabled,
            Consumer<ResourcePackDelivery> onPublished) throws IOException {
        this.logger = logger;
        this.dataDir = dataDir;
        this.publicUrl = publicUrl;
        this.packId = packId;
        this.logicalId = logicalId;
        this.enabled = enabled;
        this.onPublished = onPublished;
        this.backendExportUrl = URI.create("http://127.0.0.1:3061/api/v1/packs/export");
        this.client = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(3))
                .followRedirects(HttpClient.Redirect.NEVER).build();
        this.worker = Executors.newSingleThreadExecutor(task -> {
            Thread thread = new Thread(task, "vsmedia-pack-publisher");
            thread.setDaemon(true);
            return thread;
        });
        this.httpExecutor = Executors.newFixedThreadPool(2, task -> {
            Thread thread = new Thread(task, "vsmedia-pack-http");
            thread.setDaemon(true);
            return thread;
        });
        if (enabled) {
            if (publicUrl == null || publicUrl.getHost() == null || publicUrl.getScheme() == null) {
                throw new IllegalArgumentException("resource-pack-server.public-url debe ser una URL completa");
            }
            InetAddress bind = InetAddress.getByName(bindAddress);
            server = HttpServer.create(new InetSocketAddress(bind, port), 8);
            server.createContext("/resource-pack.zip", this::serveZip);
            server.setExecutor(httpExecutor);
            server.start();
            logger.info("VSMedia Resource Pack server: " + publicUrl);
        }
    }

    public CompletableFuture<ResourcePackDelivery> publishAsync() {
        if (!enabled) {
            return CompletableFuture.failedFuture(
                    new IllegalStateException("resource-pack-server.enabled=false"));
        }
        return CompletableFuture.supplyAsync(this::publishBlocking, worker);
    }

    public ResourcePackDelivery current() {
        return current;
    }

    public boolean enabled() {
        return enabled;
    }

    private ResourcePackDelivery publishBlocking() {
        Path folder = dataDir.resolve("resource-pack");
        Path temp = folder.resolve("current.zip.tmp");
        Path zip = folder.resolve("current.zip");
        try {
            Files.createDirectories(folder);
            HttpRequest request = HttpRequest.newBuilder(backendExportUrl)
                    .timeout(Duration.ofSeconds(120)).GET().build();
            HttpResponse<InputStream> response = client.send(
                    request, HttpResponse.BodyHandlers.ofInputStream());
            if (response.statusCode() != 200) {
                try (InputStream body = response.body()) {
                    String message = new String(body.readAllBytes(),
                            java.nio.charset.StandardCharsets.UTF_8);
                    throw new IOException("VSCore export HTTP " + response.statusCode()
                            + ": " + message.substring(0, Math.min(500, message.length())));
                }
            }
            long copied = 0;
            try (InputStream input = response.body();
                 OutputStream output = Files.newOutputStream(temp)) {
                byte[] buffer = new byte[64 * 1024];
                for (int n; (n = input.read(buffer)) != -1;) {
                    copied += n;
                    if (copied > MAX_ZIP) {
                        throw new IOException("Resource Pack supera 128 MiB");
                    }
                    output.write(buffer, 0, n);
                }
            }
            if (copied < 64 || !isZip(zipTemp(temp))) {
                throw new IOException("VSCore devolvió un ZIP vacío o inválido");
            }
            String sha1 = sha1(temp);
            atomicReplace(temp, zip);
            ResourcePackDelivery delivery = new ResourcePackDelivery(
                    logicalId, packId, publicUrl, sha1, copied);
            current = delivery;
            onPublished.accept(delivery);
            logger.info("Resource Pack publicado: " + copied + " bytes, SHA-1 " + sha1);
            return delivery;
        } catch (Exception error) {
            try { Files.deleteIfExists(temp); } catch (IOException ignored) {}
            throw new RuntimeException("No se pudo publicar el Resource Pack: "
                    + error.getMessage(), error);
        }
    }

    private static Path zipTemp(Path temp) {
        return temp;
    }

    private static boolean isZip(Path path) {
        try (ZipFile ignored = new ZipFile(path.toFile())) {
            return ignored.size() > 0;
        } catch (IOException error) {
            return false;
        }
    }

    private static String sha1(Path file) throws Exception {
        MessageDigest digest = MessageDigest.getInstance("SHA-1");
        try (InputStream input = Files.newInputStream(file)) {
            byte[] buffer = new byte[64 * 1024];
            for (int n; (n = input.read(buffer)) != -1;) digest.update(buffer, 0, n);
        }
        return HexFormat.of().formatHex(digest.digest());
    }

    private static void atomicReplace(Path from, Path to) throws IOException {
        try {
            Files.move(from, to, StandardCopyOption.REPLACE_EXISTING,
                    StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException unsupported) {
            Files.move(from, to, StandardCopyOption.REPLACE_EXISTING);
        }
    }

    private void serveZip(HttpExchange exchange) throws IOException {
        try (exchange) {
            if (!"GET".equals(exchange.getRequestMethod())
                    && !"HEAD".equals(exchange.getRequestMethod())) {
                exchange.sendResponseHeaders(405, -1);
                return;
            }
            Path file = dataDir.resolve("resource-pack/current.zip").normalize();
            if (!Files.isRegularFile(file) || current == null) {
                exchange.sendResponseHeaders(404, -1);
                return;
            }
            exchange.getResponseHeaders().set("Content-Type", "application/zip");
            exchange.getResponseHeaders().set("Cache-Control", "no-store");
            exchange.getResponseHeaders().set("X-VSMedia-SHA1", current.sha1Hex());
            exchange.getResponseHeaders().set("Content-Length", Long.toString(Files.size(file)));
            if ("HEAD".equals(exchange.getRequestMethod())) {
                exchange.sendResponseHeaders(200, -1);
                return;
            }
            exchange.sendResponseHeaders(200, Files.size(file));
            try (OutputStream out = exchange.getResponseBody();
                 InputStream input = Files.newInputStream(file)) {
                input.transferTo(out);
            }
        }
    }

    @Override
    public void close() {
        HttpServer http = server;
        server = null;
        if (http != null) http.stop(0);
        worker.shutdownNow();
        httpExecutor.shutdownNow();
    }
}
