package io.virgoland.vsmedia.api;

/**
 * Native editable documents. WebM/WebP/etc remain underlying media codecs,
 * not replacements for the project manifest formats.
 */
public enum NativeFormat {
    AUDIO("vsaudio"),
    IMAGE("vsimage"),
    STREAM("vsstream"),
    PACK("vspack"),
    PROJECT("vsproject");

    private final String extension;
    NativeFormat(String extension) { this.extension = extension; }
    public String extension() { return extension; }
}
