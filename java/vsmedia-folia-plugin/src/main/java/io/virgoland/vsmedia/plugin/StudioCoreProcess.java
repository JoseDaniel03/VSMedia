package io.virgoland.vsmedia.plugin;

import java.io.IOException;
import java.io.InputStream;
import java.net.InetSocketAddress;
import java.net.Socket;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.nio.file.StandardOpenOption;
import java.nio.file.attribute.PosixFilePermission;
import java.util.EnumSet;
import java.util.Locale;
import java.util.logging.Logger;

/**
 * Optional Linux x86_64 VSCore subprocess owned by the Folia test plugin.
 * Never executes arbitrary paths, shell commands or user-provided binaries.
 * The embedded executable comes exclusively from the trusted JAR resource.
 */
public final class StudioCoreProcess implements AutoCloseable {
    private static final String RESOURCE = "/studio-native/vsmedia-studio-bridge";
    private static final int MAX_NATIVE_BYTES = 70 * 1024 * 1024;
    private final Logger logger;
    private Process process;
    private boolean externalBridge;

    public StudioCoreProcess(Logger logger) {
        this.logger = logger;
    }

    public boolean start(Path folder) {
        if (isListening()) {
            externalBridge = true;
            logger.info("Found external VSCore on 127.0.0.1:3061; will not start another copy");
            return true;
        }
        if (!System.getProperty("os.name", "").toLowerCase(Locale.ROOT).contains("linux")
                || !("amd64".equals(System.getProperty("os.arch"))
                    || "x86_64".equals(System.getProperty("os.arch")))) {
            logger.warning("Bundled VSCore supports Linux x86_64 only. "
                    + "Start the Rust bridge manually on this platform.");
            return false;
        }
        try {
            Files.createDirectories(folder);
            Path backendFolder = folder.resolve("vsmedia-core");
            Files.createDirectories(backendFolder);
            Path binary = backendFolder.resolve("vsmedia-studio-bridge");
            Path temp = Files.createTempFile(backendFolder, ".vsmedia-core-", ".tmp");
            try {
                try (InputStream in = StudioCoreProcess.class.getResourceAsStream(RESOURCE)) {
                    if (in == null) {
                        logger.warning("Native VSCore missing from JAR. "
                                + "The Home can load but saving/rendering audio needs Rust.");
                        return false;
                    }
                    int copied = 0;
                    try (var output = Files.newOutputStream(temp,
                            StandardOpenOption.WRITE, StandardOpenOption.TRUNCATE_EXISTING)) {
                        byte[] buffer = new byte[64 * 1024];
                        for (int n; (n = in.read(buffer)) != -1;) {
                            copied += n;
                            if (copied > MAX_NATIVE_BYTES) {
                                throw new IOException("Embedded VSCore exceeds native size quota");
                            }
                            output.write(buffer, 0, n);
                        }
                    }
                    if (copied < 1024) {
                        throw new IOException("Native VSCore executable is missing or truncated");
                    }
                }
                Files.move(temp, binary, StandardCopyOption.REPLACE_EXISTING);
            } finally {
                Files.deleteIfExists(temp);
            }
            Files.setPosixFilePermissions(binary, EnumSet.of(
                    PosixFilePermission.OWNER_READ,
                    PosixFilePermission.OWNER_WRITE,
                    PosixFilePermission.OWNER_EXECUTE));
            Path data = folder.resolve("vsmedia-data");
            Files.createDirectories(data);
            Path log = folder.resolve("vsmedia-core.log");
            ProcessBuilder builder = new ProcessBuilder(binary.toAbsolutePath().toString());
            builder.environment().put("VSMEDIA_DATA_DIR", data.toAbsolutePath().toString());
            builder.directory(folder.toFile());
            builder.redirectErrorStream(true).redirectOutput(ProcessBuilder.Redirect.appendTo(log.toFile()));
            process = builder.start();
            logger.info("Starting bundled VSCore: " + binary + " (loopback 127.0.0.1:3061)");
            if (!process.isAlive()) {
                logger.warning("Native VSCore exited immediately; see " + log);
                process = null;
                return false;
            }
            return true;
        } catch (IOException | SecurityException error) {
            logger.warning("Could not start native VSCore: " + error.getMessage());
            return false;
        }
    }

    public boolean isRunning() {
        return (process != null && process.isAlive())
                || (externalBridge && isListening());
    }

    private static boolean isListening() {
        try (Socket socket = new Socket()) {
            socket.connect(new InetSocketAddress("127.0.0.1", 3061), 200);
            return true;
        } catch (IOException unreachable) {
            return false;
        }
    }

    @Override
    public void close() {
        if (process != null) {
            process.destroy();
            try {
                if (!process.waitFor(2, java.util.concurrent.TimeUnit.SECONDS)) {
                    process.destroyForcibly();
                    process.waitFor(2, java.util.concurrent.TimeUnit.SECONDS);
                }
            } catch (InterruptedException interrupted) {
                process.destroyForcibly();
                Thread.currentThread().interrupt();
            } finally {
                process = null;
            }
        }
        externalBridge = false;
    }
}
