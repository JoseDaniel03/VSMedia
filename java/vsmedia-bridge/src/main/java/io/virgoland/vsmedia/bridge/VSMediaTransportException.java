package io.virgoland.vsmedia.bridge;

/** Structured IPC error with a stable machine-readable code. */
public final class VSMediaTransportException extends RuntimeException {
    private final String code;
    public VSMediaTransportException(String code, String message) {
        super(message);
        this.code = code;
    }
    public VSMediaTransportException(String code, String message, Throwable cause) {
        super(message, cause);
        this.code = code;
    }
    public String code() { return code; }
}
