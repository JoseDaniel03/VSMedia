package io.virgoland.vsmedia.plugin;

import su.plo.voice.api.addon.AddonInitializer;
import su.plo.voice.api.addon.InjectPlasmoVoice;
import su.plo.voice.api.addon.annotation.Addon;
import su.plo.voice.api.server.PlasmoVoiceServer;
import su.plo.voice.api.server.audio.line.ServerSourceLine;

/**
 * Server-only Plasmo Voice addon. The server API sends Opus audio over the
 * already installed Plasmo Voice client mod; no standalone audio client or
 * external socket is needed. Loaded from VSMediaPlugin.onLoad().
 */
@Addon(
    id = "pv-addon-vsmedia-radio",
    name = "VSMedia Radio",
    version = "0.1.0",
    authors = {"Virgoland"}
)
public final class RadioVoiceAddon implements AddonInitializer {
    @InjectPlasmoVoice
    private PlasmoVoiceServer voiceServer;
    private volatile ServerSourceLine radioLine;

    @Override
    public void onAddonInitialize() {
        radioLine = voiceServer.getSourceLineManager()
            .createBuilder(this, "vsradio",
                "pv.activation.priority",
                "plasmovoice:textures/icons/speaker_priority.png", 9)
            .build();
    }

    @Override
    public void onAddonShutdown() {
        radioLine = null;
    }

    public boolean available() {
        return voiceServer != null && radioLine != null;
    }

    public PlasmoVoiceServer voiceServer() {
        if (!available()) throw new IllegalStateException("Plasmo Voice no está conectado");
        return voiceServer;
    }

    public ServerSourceLine line() {
        if (!available()) throw new IllegalStateException("Plasmo Voice no está conectado");
        return radioLine;
    }
}
