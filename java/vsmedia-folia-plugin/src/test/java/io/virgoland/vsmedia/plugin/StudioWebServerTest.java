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
            assertTrue(home.body().contains("VSMedia"),
                    "The JAR must serve a VSMedia frontend");
            if (web.usingV11()) {
                HttpResponse<String> caps = client.send(HttpRequest.newBuilder(
                        URI.create(root + "/api/audio/capabilities")).GET().build(),
                        HttpResponse.BodyHandlers.ofString());
                assertEquals(200, caps.statusCode());
                assertTrue(caps.body().contains("\"ffmpeg\":false"));
                assertTrue(caps.body().contains("\"pluginConnected\":true"));
            }
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
    @Test
    void minecraftActionsRequireSameOriginAndTypedControlHeader() throws Exception {
        int port=availablePort();
        java.util.concurrent.atomic.AtomicReference<String[]> received=
                new java.util.concurrent.atomic.AtomicReference<>();
        HttpClient client=HttpClient.newHttpClient();
        try (StudioWebServer web=new StudioWebServer(port,
                Logger.getLogger("vsmedia-control-test"),received::set)) {
            web.start();
            URI uri=URI.create("http://127.0.0.1:"+port+"/api/v1/minecraft/action");
            String body="action=title&player=Demo&font=minecraft%3Adefault"
                    +"&color=%23FFAACC&text=Hola+Virgoland";
            HttpRequest wrongOrigin=HttpRequest.newBuilder(uri)
                    .header("Origin","https://evil.example")
                    .header("X-VSMedia-Control","1")
                    .header("Content-Type","application/x-www-form-urlencoded")
                    .POST(HttpRequest.BodyPublishers.ofString(body)).build();
            assertEquals(403,client.send(wrongOrigin,
                    HttpResponse.BodyHandlers.ofString()).statusCode());
            assertNull(received.get());
            HttpRequest allowed=HttpRequest.newBuilder(uri)
                    .header("Origin","http://127.0.0.1:"+port)
                    .header("X-VSMedia-Control","1")
                    .header("Content-Type","application/x-www-form-urlencoded")
                    .POST(HttpRequest.BodyPublishers.ofString(body)).build();
            HttpResponse<String> queued=client.send(allowed,
                    HttpResponse.BodyHandlers.ofString());
            assertEquals(202,queued.statusCode());
            assertArrayEquals(new String[]{"title","Demo","minecraft:default",
                    "#FFAACC","Hola Virgoland"},received.get());
        }
    }

    @Test
    void proxyRejectsCrossOriginWritesAndKeepsDJSessionToken() throws Exception {
        var headers=new com.sun.net.httpserver.Headers();
        headers.add("Origin","http://127.0.0.1:3060");
        assertTrue(StudioWebServer.trustedOrigin(headers,3060));
        assertFalse(StudioWebServer.trustedOrigin(headers,3050));
        headers.set("Origin","https://attacker.example");
        assertFalse(StudioWebServer.trustedOrigin(headers,3060));
        headers.set("Origin","http://127.0.0.1:3060");
        String token="fb42bea4-2d65-4a27-80ac-b9475b6d13d0";
        headers.add("X-VSRadio-Session",token);
        var builder=HttpRequest.newBuilder(
                URI.create("http://127.0.0.1:3061/api/v1/radio/stations/demo/live/push"));
        StudioWebServer.forwardRadioToken(builder,headers);
        var built=builder.GET().build();
        assertEquals(token,built.headers().firstValue("X-VSRadio-Session").orElseThrow());
        headers.set("X-VSRadio-Session","invalid/../token");
        var rejected=HttpRequest.newBuilder(URI.create("http://127.0.0.1:3061/"));
        StudioWebServer.forwardRadioToken(rejected,headers);
        assertTrue(rejected.GET().build().headers().firstValue("X-VSRadio-Session").isEmpty());
    }

    @Test
    void studioAPIMutationsRefuseMissingOrExternalOrigin() throws Exception {
        int port=availablePort();
        HttpClient client=HttpClient.newHttpClient();
        try(StudioWebServer server=new StudioWebServer(port,
                Logger.getLogger("vsmedia-proxy-origin-test"))) {
            server.start();
            URI uri=URI.create("http://127.0.0.1:"+port+"/api/v1/radio/stations/demo/live/start");
            HttpRequest evil=HttpRequest.newBuilder(uri).POST(
                HttpRequest.BodyPublishers.ofString("{}"))
                .header("Origin","https://evil.invalid")
                .header("Content-Type","application/json").build();
            assertEquals(403,client.send(evil,HttpResponse.BodyHandlers.ofString()).statusCode());
            HttpRequest absent=HttpRequest.newBuilder(uri).POST(
                HttpRequest.BodyPublishers.ofString("{}"))
                .header("Content-Type","application/json").build();
            assertEquals(403,client.send(absent,HttpResponse.BodyHandlers.ofString()).statusCode());
        }
    }

}
