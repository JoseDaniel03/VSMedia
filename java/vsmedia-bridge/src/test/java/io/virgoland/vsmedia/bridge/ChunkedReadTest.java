package io.virgoland.vsmedia.bridge;

import com.fasterxml.jackson.databind.ObjectMapper;
import com.fasterxml.jackson.databind.node.ObjectNode;
import io.virgoland.vsmedia.api.AssetId;
import org.junit.jupiter.api.Test;

import java.io.DataInputStream;
import java.io.DataOutputStream;
import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.Socket;
import java.security.MessageDigest;
import java.util.Arrays;
import java.util.Base64;
import java.util.HexFormat;
import java.util.Random;
import java.util.concurrent.CompletionException;
import java.util.concurrent.Executors;
import java.util.concurrent.TimeUnit;

import static org.junit.jupiter.api.Assertions.*;

/**
 * Executable contract against a framed mock Rust service, including a payload
 * deliberately larger than the old 2 MiB inline transport ceiling.
 */
class ChunkedReadTest {
    @Test
    void reconstructsLargeDownloadedAudioAndClosesTransfer() throws Exception {
        exercise(false);
    }

    @Test
    void detectsCorruptedChunksEvenWhenFramingIsValid() throws Exception {
        exercise(true);
    }

    private static void exercise(boolean corrupted) throws Exception {
        byte[] original = new byte[2 * 1024 * 1024 + 13];
        new Random(27).nextBytes(original);
        String checksum = HexFormat.of().formatHex(
                MessageDigest.getInstance("SHA-256").digest(original));
        ObjectMapper json = new ObjectMapper();

        try (ServerSocket server = new ServerSocket(0, 16, InetAddress.getByName("127.0.0.1"));
             var executor = Executors.newSingleThreadExecutor();
             var client = new LoopbackVSMediaClient(server.getLocalPort(), "vsrender", "x".repeat(32))) {
            var mock = executor.submit(() -> {
                long expectedOffset = 0;
                boolean closed = false;
                while (!closed) {
                    try (Socket socket = server.accept()) {
                        socket.setSoTimeout(5000);
                        var input = new DataInputStream(socket.getInputStream());
                        int length = input.readInt();
                        assertTrue(length > 0 && length < 4 * 1024 * 1024);
                        var request = json.readTree(input.readNBytes(length));
                        assertEquals("vsrender", request.path("producer").asText());

                        var response = json.createObjectNode();
                        response.put("version", 1);
                        response.put("request_id", request.path("request_id").asText());
                        ObjectNode result = response.putObject("result");
                        var command = request.path("command");
                        switch (command.path("operation").asText()) {
                            case "begin_read" -> {
                                result.put("kind", "download");
                                result.put("transfer_id", "session-demo");
                                result.put("size", original.length);
                                result.put("sha256", checksum);
                            }
                            case "read_chunk" -> {
                                assertEquals("session-demo", command.path("transfer_id").asText());
                                assertEquals(expectedOffset, command.path("offset").asLong());
                                int end = (int) Math.min(original.length, expectedOffset + 256 * 1024);
                                byte[] piece = Arrays.copyOfRange(original, (int) expectedOffset, end);
                                if (corrupted && expectedOffset == 0) { piece[0] ^= 0x40; }
                                result.put("kind", "chunk");
                                result.put("offset", expectedOffset);
                                result.put("content_base64", Base64.getEncoder().encodeToString(piece));
                                result.put("done", end == original.length);
                                expectedOffset = end;
                            }
                            case "end_read" -> {
                                assertEquals(original.length, expectedOffset);
                                assertEquals("session-demo", command.path("transfer_id").asText());
                                result.put("kind", "acknowledged");
                                closed = true;
                            }
                            default -> fail("Unexpected IPC operation: " + command.path("operation").asText());
                        }
                        byte[] bytes = json.writeValueAsBytes(response);
                        var output = new DataOutputStream(socket.getOutputStream());
                        output.writeInt(bytes.length);
                        output.write(bytes);
                        output.flush();
                    }
                }
                return null;
            });

            AssetId id = new AssetId("demo", "recordings/large.wav");
            if (corrupted) {
                var error = assertThrows(CompletionException.class,
                        () -> client.read(id).toCompletableFuture().join());
                assertInstanceOf(VSMediaTransportException.class, error.getCause());
                assertEquals("integrity", ((VSMediaTransportException) error.getCause()).code());
            } else {
                assertArrayEquals(original,
                        client.read(id).toCompletableFuture().get(10, TimeUnit.SECONDS));
            }
            mock.get(10, TimeUnit.SECONDS);
        }
    }
}
