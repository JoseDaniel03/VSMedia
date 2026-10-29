package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.virgoland.vsmedia.api.AssetId;
import io.virgoland.vsmedia.api.AssetPayload;
import io.virgoland.vsmedia.api.AssetType;
import org.junit.jupiter.api.Test;

import java.io.ByteArrayOutputStream;
import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.security.MessageDigest;
import java.util.Base64;
import java.util.HexFormat;
import java.util.Random;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

import static org.junit.jupiter.api.Assertions.*;

class ChunkedUploadTest {
    @Test
    void sendsLargeAssetInSequentialVerifiedChunks() throws Exception {
        byte[] content = new byte[2 * 1024 * 1024 + 17];
        new Random(177).nextBytes(content);
        String digest = HexFormat.of().formatHex(MessageDigest.getInstance("SHA-256").digest(content));
        var mapper = new ObjectMapper();

        try (ServerSocket server = new ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"));
             var executor = Executors.newSingleThreadExecutor();
             var client = new LoopbackVSMediaClient(server.getLocalPort(), "vsrender", "x".repeat(32))) {
            var mock = executor.submit(() -> {
                var collected = new ByteArrayOutputStream();
                boolean done = false;
                int next = 0;
                while (!done) {
                    try (Socket socket = server.accept()) {
                        socket.setSoTimeout(5000);
                        var input = new DataInputStream(socket.getInputStream());
                        int size = input.readInt();
                        var request = mapper.readTree(input.readNBytes(size));
                        var command = request.path("command");
                        var operation = command.path("operation").asText();

                        ObjectNode response = mapper.createObjectNode();
                        response.put("version", 1);
                        response.put("request_id", request.path("request_id").asText());
                        ObjectNode result = response.putObject("result");
                        switch (operation) {
                            case "begin_upload" -> {
                                assertEquals("demo", command.path("key").path("namespace").asText());
                                assertEquals("sound", command.path("kind").asText());
                                assertEquals(content.length, command.path("size").asInt());
                                assertEquals(digest, command.path("sha256").asText());
                                result.put("kind", "upload");
                                result.put("transfer_id", "session-test");
                                result.put("offset", 0);
                            }
                            case "upload_chunk" -> {
                                assertEquals("session-test", command.path("transfer_id").asText());
                                assertEquals(next, command.path("offset").asInt());
                                byte[] chunk = Base64.getDecoder().decode(
                                        command.path("content_base64").asText());
                                assertTrue(chunk.length > 0 && chunk.length <= 256 * 1024);
                                collected.writeBytes(chunk);
                                next += chunk.length;
                                result.put("kind", "upload");
                                result.put("transfer_id", "session-test");
                                result.put("offset", next);
                            }
                            case "complete_upload" -> {
                                assertEquals(content.length, next);
                                assertArrayEquals(content, collected.toByteArray());
                                result.put("kind", "asset");
                                ObjectNode record = result.putObject("record");
                                ObjectNode key = record.putObject("key");
                                key.put("namespace", "demo");
                                key.put("name", "music/big.wav");
                                record.put("kind", "sound");
                                record.put("sha256", digest);
                                record.put("size", content.length);
                                record.put("revision", 1);
                                done = true;
                            }
                            default -> fail("Unexpected IPC command: " + operation);
                        }
                        byte[] packet = mapper.writeValueAsBytes(response);
                        var output = new DataOutputStream(socket.getOutputStream());
                        output.writeInt(packet.length);
                        output.write(packet);
                        output.flush();
                    }
                }
                return null;
            });
            var key = new AssetId("demo", "music/big.wav");
            var result = client.publish(new AssetPayload(key, AssetType.SOUND, content))
                    .toCompletableFuture().get(10, TimeUnit.SECONDS);
            assertEquals(content.length, result.size());
            assertEquals(digest, result.sha256());
            assertEquals(1, result.revision());
            mock.get(10, TimeUnit.SECONDS);
        }
    }
}
