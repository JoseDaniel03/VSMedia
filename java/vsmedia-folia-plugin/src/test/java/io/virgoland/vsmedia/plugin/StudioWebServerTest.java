package io.virgoland.vsmedia.plugin;

import java.net.InetAddress;
import java.net.ServerSocket;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.time.Duration;
import java.util.logging.Logger;
import org.junit.jupiter.api.Test;
import static org.junit.jupiter.api.Assertions.*;

/** The original Trunk distribution is included in target/classes during Maven verify. */
class StudioWebServerTest {
    private int availablePort() throws Exception {
        try (ServerSocket open=new ServerSocket(0,0,InetAddress.getByName("127.0.0.1"))) {
            return open.getLocalPort();
        }
    }
    @Test
    void servesOriginalStudioAndProtectsJarResources() throws Exception {
        int port=availablePort();
        HttpClient client=HttpClient.newBuilder()
                .connectTimeout(Duration.ofSeconds(2)).build();
        try (StudioWebServer web=new StudioWebServer(port,
                Logger.getLogger("vsmedia-test"))) {
            web.start();
            String root="http://127.0.0.1:"+port;
            HttpResponse<String> home=client.send(HttpRequest.newBuilder(
                    URI.create(root+"/")).GET().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEquals(200,home.statusCode());
            assertTrue(home.body().contains("VSMedia Studio"),
                    "The jar must serve the original Leptos Studio HTML");
            HttpResponse<String> denied=client.send(HttpRequest.newBuilder(
                    URI.create(root+"/%2e%2e/plugin.yml")).GET().build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEquals(404,denied.statusCode());
            HttpResponse<String> deniedWrite=client.send(HttpRequest.newBuilder(
                    URI.create(root+"/index.html")).POST(
                        HttpRequest.BodyPublishers.noBody()).build(),
                    HttpResponse.BodyHandlers.ofString());
            assertEquals(405,deniedWrite.statusCode());
        }
    }
}
