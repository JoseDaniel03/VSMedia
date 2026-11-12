package io.virgoland.vsmedia.plugin;

import com.sun.net.httpserver.Headers;
import com.sun.net.httpserver.HttpExchange;
import com.sun.net.httpserver.HttpServer;
import java.io.IOException;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.InetAddress;
import java.net.InetSocketAddress;
import java.net.URI;
import java.net.URLDecoder;
import java.nio.charset.StandardCharsets;
import java.util.HashMap;
import java.util.Map;
import java.util.function.Consumer;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.Locale;
import java.util.Objects;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.logging.Logger;

/**
 * Read-only packaged Leptos/WASM Studio plus a loopback-only HTTP API proxy.
 *
 * No Bukkit API or audio conversion is called on request threads. The Rust
 * bridge at 127.0.0.1:3061 is a separate process. WebSocket features such as
 * VSRadio need their own integration and are not proxied.
 */
public final class StudioWebServer implements AutoCloseable {
    private static final long MAX_BODY = 96L * 1024 * 1024;
    private final String resourcePrefix;
    private final HttpServer server;
    private final ExecutorService executor;
    private final HttpClient client;
    private final Logger logger;
    private final String url;
    private final Consumer<String[]> minecraftHandler;

    public StudioWebServer(int port, Logger logger) throws IOException {
        this(port, logger, null);
    }

    public StudioWebServer(int port, Logger logger, Consumer<String[]> minecraftHandler)
            throws IOException {
        this.minecraftHandler = minecraftHandler;
        if (port < 1024 || port > 65535) {
            throw new IllegalArgumentException("Port must be 1024..65535");
        }
        this.logger = Objects.requireNonNull(logger);
        this.resourcePrefix = StudioWebServer.class.getResource("/studio-web-v11/index.html") != null
                ? "/studio-web-v11/" : "/studio-web/";
        this.server = HttpServer.create(new InetSocketAddress(
                InetAddress.getByName("127.0.0.1"), port), 24);
        this.executor = Executors.newFixedThreadPool(4, task -> {
            Thread thread = new Thread(task, "vsmedia-studio-http");
            thread.setDaemon(true);
            return thread;
        });
        this.server.setExecutor(executor);
        this.client = HttpClient.newBuilder().connectTimeout(Duration.ofSeconds(3))
                .followRedirects(HttpClient.Redirect.NEVER).build();
        this.server.createContext("/", this::dispatch);
        this.url = "http://127.0.0.1:" + port + "/";
    }

    public void start() {
        server.start();
        logger.info("VSMedia Web " + (usingV11() ? "v11" : "legacy FullCanvas") + ": " + url);
        if (!usingV11()) logger.warning("La web v11 todavía no está empaquetada en el JAR. "
                + "Se sirve el frontend FullCanvas anterior.");
    }

    public String url() {
        return url;
    }

    public boolean usingV11() {
        return "/studio-web-v11/".equals(resourcePrefix);
    }

    private void dispatch(HttpExchange exchange) throws IOException {
        try (exchange) {
            Headers out = exchange.getResponseHeaders();
            out.set("X-Content-Type-Options", "nosniff");
            out.set("Cache-Control", "no-store");
            out.set("Referrer-Policy", "no-referrer");
            String path = exchange.getRequestURI().getPath();
            if (path.equals("/api/audio/capabilities") && usingV11()) {
                if (!"GET".equals(exchange.getRequestMethod())) {
                    fail(exchange, 405, "Method not allowed");
                } else {
                    byte[] payload = ("{\"ok\":true,\"server\":\"folia-static\","
                            + "\"ffmpeg\":false,\"browserEncoding\":true,"
                            + "\"pluginConnected\":true}").getBytes(java.nio.charset.StandardCharsets.UTF_8);
                    out.set("Content-Type", "application/json; charset=utf-8");
                    exchange.sendResponseHeaders(200, payload.length);
                    try (OutputStream body = exchange.getResponseBody()) { body.write(payload); }
                }
            } else if (path.startsWith("/api/audio/") && usingV11()) {
                // Deliberately no FFmpeg on the Minecraft server. Use browser encoders.
                fail(exchange, 501, "Conversion on Folia is disabled; encode in the browser");
            } else if (path.equals("/api/v1/minecraft/action")) {
                minecraftAction(exchange);
            } else if (path.startsWith("/api/v1/")) {
                proxy(exchange, path);
            } else {
                staticResource(exchange, path);
            }
        } catch (InterruptedException interrupted) {
            Thread.currentThread().interrupt();
            fail(exchange, 503, "Studio request interrupted");
        } catch (RuntimeException invalid) {
            logger.warning("Studio HTTP request failed: "
                    + invalid.getClass().getSimpleName());
            fail(exchange, 500, "Studio server error");
        }
    }

    /**
     * Explicit same-origin local operator action. Never call the Bukkit API
     * on the HTTP executor; the plugin callback schedules on Folia's global
     * region and each player subsequently on its owning entity scheduler.
     */
    private void minecraftAction(HttpExchange exchange) throws IOException {
        if (!"POST".equals(exchange.getRequestMethod())) {
            fail(exchange, 405, "Method not allowed"); return;
        }
        if (minecraftHandler == null) {
            fail(exchange, 503, "Minecraft actions are unavailable"); return;
        }
        String origin = exchange.getRequestHeaders().getFirst("Origin");
        int port = server.getAddress().getPort();
        if (origin == null || !(origin.equals("http://127.0.0.1:" + port)
                || origin.equals("http://localhost:" + port))
                || !"1".equals(exchange.getRequestHeaders().getFirst("X-VSMedia-Control"))) {
            fail(exchange, 403, "Same-origin local operator action required"); return;
        }
        String contentType = exchange.getRequestHeaders().getFirst("Content-Type");
        if (contentType == null || !contentType.startsWith("application/x-www-form-urlencoded")) {
            fail(exchange, 415, "Expected form-urlencoded body"); return;
        }
        byte[] bytes;
        try (InputStream input = exchange.getRequestBody()) {
            bytes = input.readNBytes(4097);
        }
        if (bytes.length > 4096) {
            fail(exchange, 413, "Command payload too large"); return;
        }
        Map<String,String> params = new HashMap<>();
        try {
            for (String entry : new String(bytes, StandardCharsets.UTF_8).split("&")) {
                String[] parts = entry.split("=", 2);
                if (parts.length != 2 || params.size() >= 8) throw new IllegalArgumentException();
                String key = URLDecoder.decode(parts[0], StandardCharsets.UTF_8);
                String value = URLDecoder.decode(parts[1], StandardCharsets.UTF_8);
                if (value.length() > 512 || value.chars().anyMatch(ch -> ch < 32)
                        || params.putIfAbsent(key, value) != null) throw new IllegalArgumentException();
            }
        } catch (IllegalArgumentException invalid) {
            fail(exchange, 400, "Invalid control payload"); return;
        }
        String action = params.getOrDefault("action", "");
        String target = params.getOrDefault("player", "");
        if (!target.matches("(\\*|[A-Za-z0-9_]{1,16})")) {
            fail(exchange, 400, "Invalid player selector"); return;
        }
        String[] args;
        switch (action) {
            case "title", "subtitle", "preview" -> {
                String text = params.getOrDefault("text", "");
                if (text.isBlank()) { fail(exchange, 400, "Missing text"); return; }
                args = new String[]{action,target,
                        params.getOrDefault("font", "minecraft:default"),
                        params.getOrDefault("color", "white"),text};
            }
            case "play", "sound" -> args = new String[]{"play",target,
                    params.getOrDefault("sound", ""),
                    params.getOrDefault("volume", "1"),
                    params.getOrDefault("pitch", "1")};
            case "stopsound" -> args = new String[]{"stopsound",target,
                    params.getOrDefault("sound", "")};
            case "chatfont" -> {
                if ("*".equals(target)) {fail(exchange, 400, "Choose a player");return;}
                args = new String[]{"chatfont",target,
                        params.getOrDefault("font", "default")};
            }
            default -> { fail(exchange, 400, "Unknown control action"); return; }
        }
        try {
            minecraftHandler.accept(args);
        } catch (IllegalStateException shuttingDown) {
            fail(exchange, 503, "Minecraft scheduler unavailable");return;
        }
        byte[] result = "{\"scheduled\":true,\"status\":\"accepted-not-confirmed\"}"
                .getBytes(StandardCharsets.UTF_8);
        exchange.getResponseHeaders().set("Content-Type", "application/json; charset=utf-8");
        exchange.sendResponseHeaders(202, result.length);
        try (OutputStream output = exchange.getResponseBody()) {output.write(result);}
    }

    private void staticResource(HttpExchange exchange, String path) throws IOException {
        String method = exchange.getRequestMethod();
        if (!"GET".equals(method) && !"HEAD".equals(method)) {
            fail(exchange, 405, "Method not allowed");
            return;
        }
        if (path.equals("/")) {
            path = "/index.html";
        }
        // Only web assets produced by Trunk: no traversal, decoded escapes,
        // symbolic paths, or access to other JAR contents.
        if (!path.matches("/[A-Za-z0-9_./\\- ]{1,180}") || path.contains("..")
                || path.contains("//") || path.endsWith("/")) {
            fail(exchange, 404, "Not found");
            return;
        }
        String name = resourcePrefix + path.substring(1);
        java.net.URL resource = getClass().getResource(name);
        if (resource == null) {
            fail(exchange, 404, "Not found");
            return;
        }
        java.net.URLConnection connection = resource.openConnection();
        connection.setUseCaches(false);
        long bytes = connection.getContentLengthLong();
        if (bytes > 32L * 1024 * 1024) {
            fail(exchange, 413, "Static resource too large");
            return;
        }
        exchange.getResponseHeaders().set("Content-Type", mime(path));
        // Do not keep large Blockbench bundles in the JVM heap.
        exchange.getResponseHeaders().set("Cache-Control", path.endsWith(".html")
                ? "no-cache" : "private, max-age=120");
        if ("HEAD".equals(method)) {
            exchange.sendResponseHeaders(200, -1);
            return;
        }
        try (InputStream input = connection.getInputStream()) {
            exchange.sendResponseHeaders(200, bytes > 0 ? bytes : 0);
            try (OutputStream output = exchange.getResponseBody()) {
                byte[] buffer = new byte[64 * 1024];
                long copied = 0;
                for (int read; (read = input.read(buffer)) != -1; ) {
                    copied += read;
                    if (copied > 32L * 1024 * 1024) throw new IOException("Static resource exceeded limit");
                    output.write(buffer, 0, read);
                }
            }
        }
    }

    private void proxy(HttpExchange exchange, String path)
            throws IOException, InterruptedException {
        String method = exchange.getRequestMethod();
        if (!"GET".equals(method) && !"POST".equals(method)
                && !"DELETE".equals(method)) {
            fail(exchange, 405, "Method not allowed");
            return;
        }
        // The Rust API is only reachable on the same machine; external
        // browser access is through an operator-controlled SSH tunnel.
        URI requested = exchange.getRequestURI();
        String query = requested.getRawQuery();
        URI backend = URI.create("http://127.0.0.1:3061"
                + requested.getRawPath() + (query == null ? "" : "?" + query));
        if (exchange.getRequestHeaders().getFirst("Upgrade") != null) {
            fail(exchange, 501, "WebSocket proxy not implemented");
            return;
        }
        long claimed = 0;
        String contentLength = exchange.getRequestHeaders().getFirst("Content-Length");
        if (contentLength != null) {
            try {
                claimed = Long.parseLong(contentLength);
            } catch (NumberFormatException bad) {
                fail(exchange, 400, "Invalid Content-Length");
                return;
            }
        }
        if (claimed < 0 || claimed > MAX_BODY) {
            fail(exchange, 413, "Upload too large");
            return;
        }
        byte[] body;
        try (InputStream source = exchange.getRequestBody()) {
            body = source.readNBytes((int) MAX_BODY + 1);
        }
        if (body.length > MAX_BODY) {
            fail(exchange, 413, "Upload too large");
            return;
        }
        HttpRequest.Builder builder = HttpRequest.newBuilder(backend)
                .timeout(Duration.ofSeconds(90))
                .method(method, HttpRequest.BodyPublishers.ofByteArray(body));
        String contentType = exchange.getRequestHeaders().getFirst("Content-Type");
        if (contentType != null && contentType.length() <= 180
                && !contentType.contains("\r") && !contentType.contains("\n")) {
            builder.header("Content-Type", contentType);
        }
        String label = exchange.getRequestHeaders().getFirst("X-VSPack-Label");
        if (label != null && label.length() <= 160
                && !label.contains("\r") && !label.contains("\n")) {
            builder.header("X-VSPack-Label", label);
        }
        HttpResponse<InputStream> upstream;
        try {
            upstream = client.send(builder.build(), HttpResponse.BodyHandlers.ofInputStream());
        } catch (java.net.ConnectException unavailable) {
            fail(exchange, 503, "Rust VSCore is not running on 127.0.0.1:3061");
            return;
        } catch (java.net.http.HttpTimeoutException timeout) {
            fail(exchange, 504, "Rust VSCore request timed out");
            return;
        }
        try (InputStream incoming = upstream.body()) {
            for (String header : new String[]{"Content-Type", "Content-Disposition",
                    "ETag", "X-VSPack-SHA1", "X-VSPack-SHA256"}) {
                upstream.headers().firstValue(header).ifPresent(value -> {
                    if (value.length() < 1024) {
                        exchange.getResponseHeaders().set(header, value);
                    }
                });
            }
            // A streaming response supports WAV renders and large pack ZIPs.
            // chunked transfer rather than buffering an entire rendered file.
            exchange.sendResponseHeaders(upstream.statusCode(), 0);
            try (OutputStream out = exchange.getResponseBody()) {
                incoming.transferTo(out);
            }
        }
    }

    private static String mime(String path) {
        String lower = path.toLowerCase(Locale.ROOT);
        if (lower.endsWith(".html")) return "text/html; charset=utf-8";
        if (lower.endsWith(".js") || lower.endsWith(".mjs")) return "text/javascript; charset=utf-8";
        if (lower.endsWith(".webmanifest")) return "application/manifest+json";
        if (lower.endsWith(".woff2")) return "font/woff2";
        if (lower.endsWith(".woff")) return "font/woff";
        if (lower.endsWith(".ttf")) return "font/ttf";
        if (lower.endsWith(".ico")) return "image/x-icon";
        if (lower.endsWith(".jpeg") || lower.endsWith(".jpg")) return "image/jpeg";
        if (lower.endsWith(".css")) return "text/css; charset=utf-8";
        if (lower.endsWith(".wasm")) return "application/wasm";
        if (lower.endsWith(".svg")) return "image/svg+xml";
        if (lower.endsWith(".png")) return "image/png";
        if (lower.endsWith(".webp")) return "image/webp";
        if (lower.endsWith(".json")) return "application/json";
        return "application/octet-stream";
    }

    private static void fail(HttpExchange exchange, int code, String message)
            throws IOException {
        byte[] bytes = message.getBytes(java.nio.charset.StandardCharsets.UTF_8);
        try {
            exchange.getResponseHeaders().set("Content-Type", "text/plain; charset=utf-8");
            exchange.sendResponseHeaders(code, bytes.length);
            try (OutputStream out = exchange.getResponseBody()) {
                out.write(bytes);
            }
        } catch (IOException alreadyClosed) {
            // Nothing more can be sent once a stream failed.
        }
    }

    @Override
    public void close() {
        server.stop(0);
        executor.shutdownNow();
    }
}
