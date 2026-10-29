package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.JsonNode;
import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.virgoland.vsmedia.api.AssetId;
import io.virgoland.vsmedia.api.AssetPayload;
import io.virgoland.vsmedia.api.AssetReceipt;
import io.virgoland.vsmedia.api.AssetType;
import io.virgoland.vsmedia.api.NativeFormat;
import io.virgoland.vsmedia.api.NativeFormatsApi;
import io.virgoland.vsmedia.api.PackReceipt;
import io.virgoland.vsmedia.api.VSMediaApi;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
import java.util.Locale;
import java.util.Objects;
import java.util.Optional;
import java.util.UUID;
import java.util.concurrent.ArrayBlockingQueue;
import java.util.concurrent.CompletableFuture;
import java.util.concurrent.CompletionStage;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.ThreadPoolExecutor;
import java.util.concurrent.TimeUnit;
import java.util.function.Supplier;

/**
 * Async Java -> Rust v1 IPC adapter. Never call blocking socket I/O on a Folia region thread.
 * No Minecraft or plugin-producer dependencies. Socket target is fixed to loopback.
 */
public final class LoopbackVSMediaClient implements VSMediaApi, NativeFormatsApi, AutoCloseable {
    private static final int MAX_FRAME = 4 * 1024 * 1024;
    private static final int MAX_INLINE = 2 * 1024 * 1024;
    private final int port;
    private final String producer;
    private final String token;
    private final ObjectMapper json = new ObjectMapper();
    private final ExecutorService executor = new ThreadPoolExecutor(
            2, 2, 0, TimeUnit.SECONDS, new ArrayBlockingQueue<>(32),
            runnable -> {
                Thread thread = new Thread(runnable, "vsmedia-ipc");
                thread.setDaemon(true);
                return thread;
            },
            new ThreadPoolExecutor.AbortPolicy());

    public LoopbackVSMediaClient(int port, String producer, String token) {
        if (port < 1 || port > 65535 || producer == null
                || !producer.matches("[a-z0-9_-]{1,64}")
                || token == null || token.length() < 24 || token.length() > 256) {
            throw new IllegalArgumentException("Invalid VSMedia IPC configuration");
        }
        this.port = port;
        this.producer = producer;
        this.token = token;
    }
    @Override
    public CompletionStage<AssetReceipt> publish(AssetPayload payload) {
        Objects.requireNonNull(payload);
        byte[] bytes = payload.data();
        if (bytes.length > MAX_INLINE) {
            return CompletableFuture.failedFuture(new IllegalArgumentException("IPC inline resource exceeds 2 MiB"));
        }
        return async(() -> {
            ObjectNode command = command("publish", payload.id());
            command.put("kind", toWireType(payload.type()));
            command.put("content_base64", Base64.getEncoder().encodeToString(bytes));
            return toReceipt(send(command).path("record"));
        });
    }
    @Override
    public CompletionStage<Optional<AssetReceipt>> lookup(AssetId id) {
        Objects.requireNonNull(id);
        return async(() -> {
            try {
                return Optional.of(toReceipt(send(command("lookup", id)).path("record")));
            } catch (VSMediaTransportException error) {
                if ("not_found".equals(error.code())) { return Optional.empty(); }
                throw error;
            }
        });
    }
    @Override
    public CompletionStage<byte[]> read(AssetId id) {
        Objects.requireNonNull(id);
        return async(() -> {
            String encoded = send(command("read", id)).path("content_base64").asText();
            byte[] bytes = Base64.getDecoder().decode(encoded);
            if (bytes.length > MAX_INLINE) {
                throw new VSMediaTransportException("too_large", "Response exceeds IPC inline limit");
            }
            return bytes;
        });
    }
    @Override
    public CompletionStage<AssetReceipt> publishNative(
            AssetId id, NativeFormat format, byte[] manifestJson) {
        Objects.requireNonNull(id);
        Objects.requireNonNull(format);
        Objects.requireNonNull(manifestJson);
        if (manifestJson.length > MAX_INLINE) {
            return CompletableFuture.failedFuture(new IllegalArgumentException("Native manifest too large"));
        }
        // Do not block Folia. Rust validates schema/version/magic and extension.
        byte[] snapshot = manifestJson.clone();
        return async(() -> {
            ObjectNode cmd = command("publish_native", id);
            cmd.put("extension", format.extension());
            cmd.put("content_base64", Base64.getEncoder().encodeToString(snapshot));
            return toReceipt(send(cmd).path("record"));
        });
    }
    @Override
    public CompletionStage<byte[]> readNative(AssetId id, NativeFormat format) {
        Objects.requireNonNull(id);
        Objects.requireNonNull(format);
        return async(() -> {
            ObjectNode cmd = command("read_native", id);
            cmd.put("extension", format.extension());
            byte[] bytes = Base64.getDecoder().decode(send(cmd).path("content_base64").asText());
            if (bytes.length > MAX_INLINE) {
                throw new VSMediaTransportException("too_large","Native manifest exceeds IPC limit");
            }
            return bytes;
        });
    }
    @Override
    public CompletionStage<PackReceipt> buildResourcePack() {
        return CompletableFuture.failedFuture(new UnsupportedOperationException(
                "Pack compilation command is not yet implemented by the Rust v1 IPC protocol"));
    }
    public CompletionStage<Boolean> ping() {
        return async(() -> "pong".equals(send(simpleCommand("ping")).path("kind").asText()));
    }
    private <T> CompletionStage<T> async(Supplier<T> callback) {
        return CompletableFuture.supplyAsync(callback, executor);
    }
    private ObjectNode simpleCommand(String operation) {
        ObjectNode cmd = json.createObjectNode();
        cmd.put("operation", operation);
        return cmd;
    }
    private ObjectNode command(String operation, AssetId id) {
        ObjectNode cmd = simpleCommand(operation);
        ObjectNode key = cmd.putObject("key");
        key.put("namespace", id.namespace());
        key.put("name", id.name());
        return cmd;
    }
    private String toWireType(AssetType type) {
        return type.name().toLowerCase(Locale.ROOT);
    }
    private AssetReceipt toReceipt(JsonNode data) {
        JsonNode key = data.path("key");
        if (key.isMissingNode() || data.path("revision").isMissingNode()) {
            throw new VSMediaTransportException("protocol", "Missing receipt fields");
        }
        return new AssetReceipt(
                new AssetId(key.path("namespace").asText(), key.path("name").asText()),
                AssetType.valueOf(data.path("kind").asText().toUpperCase(Locale.ROOT)),
                data.path("sha256").asText(),
                data.path("size").asLong(),
                data.path("revision").asLong());
    }
    private JsonNode send(ObjectNode command) {
        String requestId = UUID.randomUUID().toString();
        ObjectNode request = json.createObjectNode();
        request.put("version", API_VERSION);
        request.put("request_id", requestId);
        request.put("producer", producer);
        request.put("token", token);
        request.set("command", command);
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress("127.0.0.1", port), 3000);
            socket.setSoTimeout(10000);
            byte[] raw = json.writeValueAsBytes(request);
            if (raw.length > MAX_FRAME) {
                throw new VSMediaTransportException("too_large", "Request exceeds frame size");
            }
            DataOutputStream output = new DataOutputStream(socket.getOutputStream());
            output.writeInt(raw.length);
            output.write(raw);
            output.flush();

            DataInputStream input = new DataInputStream(socket.getInputStream());
            int size = input.readInt();
            if (size <= 0 || size > MAX_FRAME) {
                throw new VSMediaTransportException("protocol", "Invalid response frame size");
            }
            JsonNode response = json.readTree(input.readNBytes(size));
            if (response == null || response.path("version").asInt() != API_VERSION
                    || !requestId.equals(response.path("request_id").asText())) {
                throw new VSMediaTransportException("protocol", "Response ID or version mismatch");
            }
            JsonNode error = response.path("error");
            if (!error.isMissingNode() && !error.isNull()) {
                throw new VSMediaTransportException(error.path("code").asText(), error.path("message").asText());
            }
            JsonNode result = response.path("result");
            if (result.isMissingNode() || result.isNull()) {
                throw new VSMediaTransportException("protocol", "Missing result");
            }
            return result;
        } catch (IOException error) {
            throw new VSMediaTransportException("io", "Unable to communicate with local VSMedia engine", error);
        }
    }
    @Override
    public void close() {
        executor.shutdownNow();
    }
}
