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

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.io.IOException;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.security.MessageDigest;
import java.util.HexFormat;
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
        return async(() -> readByChunks(id));
    }

    /**
     * One verified source read on the Rust side, then bounded sequential
     * chunks. The digest is checked again on Java to detect IPC corruption.
     * This method is executed only on the client's bounded worker pool.
     */
    private byte[] readByChunks(AssetId id) {
        final int maxAsset = 64 * 1024 * 1024;
        final int chunkLimit = 256 * 1024;
        JsonNode opened = send(command("begin_read", id));
        if (!"download".equals(opened.path("kind").asText())) {
            throw new VSMediaTransportException("protocol", "Expected download session");
        }
        long expectedSize = opened.path("size").asLong(-1);
        String transferId = opened.path("transfer_id").asText();
        String expectedSha = opened.path("sha256").asText();
        if (expectedSize < 0 || expectedSize > maxAsset ||
                transferId.isBlank() || expectedSha.length() != 64) {
            throw new VSMediaTransportException("protocol", "Invalid download metadata");
        }
        byte[] expectedDigest;
        try {
            expectedDigest = HexFormat.of().parseHex(expectedSha);
        } catch (IllegalArgumentException e) {
            throw new VSMediaTransportException("protocol", "Invalid content checksum", e);
        }
        try {
            MessageDigest digest = MessageDigest.getInstance("SHA-256");
            ByteArrayOutputStream buffer = new ByteArrayOutputStream((int) expectedSize);
            long offset = 0;
            boolean finished;
            do {
                ObjectNode request = simpleCommand("read_chunk");
                request.put("transfer_id", transferId);
                request.put("offset", offset);
                JsonNode response = send(request);
                if (!"chunk".equals(response.path("kind").asText()) ||
                        response.path("offset").asLong(-1) != offset) {
                    throw new VSMediaTransportException("protocol", "Unexpected download chunk");
                }
                byte[] bytes = Base64.getDecoder().decode(response.path("content_base64").asText());
                finished = response.path("done").asBoolean(false);
                if (bytes.length > chunkLimit || (bytes.length == 0 && !finished) ||
                        bytes.length > expectedSize - offset) {
                    throw new VSMediaTransportException("protocol", "Invalid download chunk length");
                }
                digest.update(bytes);
                buffer.writeBytes(bytes);
                offset += bytes.length;
            } while (!finished);
            if (offset != expectedSize || !MessageDigest.isEqual(digest.digest(), expectedDigest)) {
                throw new VSMediaTransportException("integrity", "Download size or SHA-256 mismatch");
            }
            return buffer.toByteArray();
        } catch (java.security.NoSuchAlgorithmException e) {
            throw new VSMediaTransportException("internal", "SHA-256 unavailable", e);
        } finally {
            ObjectNode close = simpleCommand("end_read");
            close.put("transfer_id", transferId);
            try {
                send(close);
            } catch (VSMediaTransportException ignored) {
                // The server also expires abandoned transfers after inactivity.
            }
        }
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
            byte[] received = input.readNBytes(size);
            if (received.length != size) {
                throw new VSMediaTransportException("protocol", "Truncated response frame");
            }
            JsonNode response = json.readTree(received);
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
