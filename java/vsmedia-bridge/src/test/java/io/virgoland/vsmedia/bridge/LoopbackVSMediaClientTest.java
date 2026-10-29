package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.virgoland.vsmedia.api.AssetId;
import io.virgoland.vsmedia.api.AssetType;
import io.virgoland.vsmedia.api.NativeFormat;
import java.nio.charset.StandardCharsets;
import java.util.Base64;
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

    @Test
    void nativeProjectCommandsSendTypedExtensionAndManifest() throws Exception {
        ObjectMapper json = new ObjectMapper();
        String raw = """
                {"magic":"VSMEDIA","version":1,"payload":{"kind":"project","data":{"name":"Adventure","nodes":[],"entry_points":[]}}}
                """;
        byte[] manifest = raw.getBytes(StandardCharsets.UTF_8);
        try (ServerSocket server = new ServerSocket(0, 2, InetAddress.getLoopbackAddress());
             var executor = Executors.newSingleThreadExecutor();
             LoopbackVSMediaClient client =
                     new LoopbackVSMediaClient(server.getLocalPort(), "demo", "x".repeat(32))) {
            var serverTask = executor.submit(() -> {
                for (int i = 0; i < 2; i++) {
                    try (Socket socket = server.accept()) {
                        var input = new DataInputStream(socket.getInputStream());
                        var request = json.readTree(input.readNBytes(input.readInt()));
                        var command = request.path("command");
                        assertEquals("vsproject", command.path("extension").asText());
                        var response = json.createObjectNode();
                        response.put("version", 1);
                        response.put("request_id", request.path("request_id").asText());
                        var result = response.putObject("result");
                        if (i == 0) {
                            assertEquals("publish_native",command.path("operation").asText());
                            assertArrayEquals(manifest,
                                    Base64.getDecoder().decode(command.path("content_base64").asText()));
                            result.put("kind","asset");
                            var record = result.putObject("record");
                            var key = record.putObject("key");
                            key.put("namespace","demo");
                            key.put("name","project/scene");
                            record.put("kind","other");
                            record.put("sha256","b".repeat(64));
                            record.put("size",manifest.length);
                            record.put("revision",1);
                        } else {
                            assertEquals("read_native",command.path("operation").asText());
                            result.put("kind","content");
                            result.put("content_base64",Base64.getEncoder().encodeToString(manifest));
                        }
                        byte[] data = json.writeValueAsBytes(response);
                        var output = new DataOutputStream(socket.getOutputStream());
                        output.writeInt(data.length);
                        output.write(data);
                        output.flush();
                    }
                }
                return null;
            });
            AssetId key = new AssetId("demo","project/scene");
            var saved = client.publishNative(key,NativeFormat.PROJECT,manifest)
                    .toCompletableFuture().get(5,TimeUnit.SECONDS);
            assertEquals(AssetType.OTHER,saved.type());
            byte[] restored = client.readNative(key,NativeFormat.PROJECT)
                    .toCompletableFuture().get(5,TimeUnit.SECONDS);
            assertArrayEquals(manifest,restored);
            serverTask.get(5,TimeUnit.SECONDS);
        }
    }
}
