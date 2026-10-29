package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.virgoland.vsmedia.api.AssetId;
import io.virgoland.vsmedia.api.AssetType;
import org.junit.jupiter.api.Test;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.util.concurrent.CompletionException;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

import static org.junit.jupiter.api.Assertions.*;

class LoopbackVSMediaClientTest {
    @Test
    void validatesOptionsAndDoesNotPretendPackBuildExists() {
        assertThrows(IllegalArgumentException.class, () -> new LoopbackVSMediaClient(0, "demo", "x".repeat(32)));
        try (LoopbackVSMediaClient client = new LoopbackVSMediaClient(3059, "demo", "x".repeat(32))) {
            CompletionException error = assertThrows(CompletionException.class,
                    () -> client.buildResourcePack().toCompletableFuture().join());
            assertInstanceOf(UnsupportedOperationException.class, error.getCause());
        }
    }
    @Test
    void sendsLengthPrefixedJsonAndMatchesResponse() throws Exception {
        ObjectMapper json = new ObjectMapper();
        try (ServerSocket server = new ServerSocket(0, 1, InetAddress.getLoopbackAddress());
             var executor = Executors.newSingleThreadExecutor();
             LoopbackVSMediaClient client = new LoopbackVSMediaClient(server.getLocalPort(), "demo", "x".repeat(32))) {
            var serverTask = executor.submit(() -> {
                try (Socket socket = server.accept()) {
                    DataInputStream input = new DataInputStream(socket.getInputStream());
                    int length = input.readInt();
                    ObjectNode req = (ObjectNode) json.readTree(input.readNBytes(length));
                    assertEquals("lookup", req.path("command").path("operation").asText());
                    assertEquals("demo", req.path("producer").asText());
                    ObjectNode response = json.createObjectNode();
                    response.put("version", 1);
                    response.put("request_id", req.path("request_id").asText());
                    ObjectNode result = response.putObject("result");
                    result.put("kind", "asset");
                    ObjectNode record = result.putObject("record");
                    ObjectNode key = record.putObject("key");
                    key.put("namespace", "demo");
                    key.put("name", "sound/test");
                    record.put("kind", "sound");
                    record.put("sha256", "abc");
                    record.put("size", 3);
                    record.put("revision", 1);
                    byte[] bytes = json.writeValueAsBytes(response);
                    DataOutputStream output = new DataOutputStream(socket.getOutputStream());
                    output.writeInt(bytes.length);
                    output.write(bytes);
                    output.flush();
                }
                return null;
            });
            var record = client.lookup(new AssetId("demo", "sound/test"))
                    .toCompletableFuture().get(5, TimeUnit.SECONDS).orElseThrow();
            assertEquals(AssetType.SOUND, record.type());
            assertEquals(1L, record.revision());
            serverTask.get(5, TimeUnit.SECONDS);
        }
    }
}
