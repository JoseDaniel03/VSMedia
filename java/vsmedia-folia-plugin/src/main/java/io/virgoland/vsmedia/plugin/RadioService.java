package io.virgoland.vsmedia.plugin;

import com.google.gson.JsonArray;
import com.google.gson.JsonObject;
import com.google.gson.JsonParser;
import java.io.IOException;
import java.net.URI;
import java.net.http.HttpClient;
import java.net.http.HttpRequest;
import java.net.http.HttpResponse;
import java.nio.charset.StandardCharsets;
import java.text.Normalizer;
import java.time.Duration;
import java.util.ArrayList;
import java.util.HashMap;
import java.util.HashSet;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Objects;
import java.util.Set;
import java.util.UUID;
import java.util.concurrent.Executors;
import java.util.concurrent.ScheduledExecutorService;
import java.util.concurrent.TimeUnit;
import java.util.logging.Logger;
import org.bukkit.ChatColor;
import org.bukkit.command.CommandSender;
import org.bukkit.entity.Player;
import su.plo.voice.api.server.audio.provider.ArrayAudioFrameProvider;
import su.plo.voice.api.server.audio.source.AudioSender;
import su.plo.voice.api.server.audio.source.ServerBroadcastSource;
import su.plo.voice.api.server.player.VoicePlayer;

/**
 * Server-controlled radio tuner. A station has ONE broadcast source and
 * all tuned listeners hear its currently streaming 20ms frames. Late listeners
 * join the source at the live position, never at the beginning of the track.
 *
 * All network requests, PCM conversion and Plasmo senders are off Folia
 * threads. Only player chat messages use the owning entity scheduler.
 * Stations/track timing are authoritative in the Rust VSCore SQLite API.
 */
final class RadioService implements AutoCloseable {
    private static final int MAX_ACTIVE_STATIONS = 8;
    private final int maxActiveStations;
    private static final int MAX_PCM_BYTES = 32 * 1024 * 1024;
    private static final URI CORE = URI.create("http://127.0.0.1:3061");
    private final VSMediaPlugin plugin;
    private final RadioVoiceAddon addon;
    private final Logger logger;
    private final HttpClient http = HttpClient.newBuilder()
        .connectTimeout(Duration.ofSeconds(3))
        .followRedirects(HttpClient.Redirect.NEVER).build();
    private final ScheduledExecutorService tasks = Executors.newSingleThreadScheduledExecutor(r -> {
        Thread t = new Thread(r, "vsmedia-radio-audio");
        t.setDaemon(true);
        return t;
    });
    /** These two maps are confined to the radio worker thread. */
    private final Map<UUID,String> tuning = new HashMap<>();
    private final Map<String,StationAudio> broadcasts = new HashMap<>();

    private static final class StationAudio {
        final String station;
        final Map<UUID,String> players = new HashMap<>();
        long trackStarted = Long.MIN_VALUE;
        String revision = "";
        String track = "";
        ServerBroadcastSource source;
        AudioSender sender;
        long lastWarning;
        StationAudio(String station) {this.station = station;}
    }

    private record Now(String station, String displayName, String track, String revision,
                       long positionMs, long trackStartedMs, long serverUnixMs) {}

    RadioService(VSMediaPlugin plugin,RadioVoiceAddon addon) {
        this.plugin=Objects.requireNonNull(plugin);
        this.addon=Objects.requireNonNull(addon);
        this.logger=plugin.getLogger();
        this.maxActiveStations=Math.max(1,Math.min(MAX_ACTIVE_STATIONS,
            plugin.getConfig().getInt("radio.max-active-stations", MAX_ACTIVE_STATIONS)));
        tasks.scheduleWithFixedDelay(this::tickSafely, 250L, 250L, TimeUnit.MILLISECONDS);
    }

    static String stationId(String name) {
        if (name == null) return "";
        String lower=Normalizer.normalize(name,Normalizer.Form.NFD)
            .replaceAll("\\p{M}", "").toLowerCase(Locale.ROOT)
            .replaceAll("[^a-z0-9_-]", "");
        return lower.length()<=48?lower:lower.substring(0,48);
    }

    static int sampleOffset(long positionMs,long serverUnixMs,long localUnixMs,int sampleCount) {
        // A status request may be in-flight for tens or hundreds of milliseconds.
        // Advance by that time (bounded) instead of replaying the older instant.
        long latency=Math.max(0L,Math.min(2000L,localUnixMs-serverUnixMs));
        long samples=Math.max(0L,positionMs+latency)*48L;
        return (int)Math.min((long)sampleCount, samples);
    }

    private void tell(Player player,String message) {
        player.getScheduler().run(plugin, task -> {
            if(player.isOnline())player.sendMessage(message);
        }, () -> {});
    }

    boolean command(CommandSender sender,String[] args) {
        if(!(sender instanceof Player player)){
            sender.sendMessage(ChatColor.RED+"VSRadio se sintoniza desde el juego, no desde consola.");
            return true;
        }
        String action=args.length==0?"ayuda":args[0].toLowerCase(Locale.ROOT);
        switch(action) {
            case "emisora" -> {
                if(args.length!=2){
                    player.sendMessage(ChatColor.YELLOW+"Uso: /vsradio emisora LatinSabroso");
                    return true;
                }
                String station=stationId(args[1]);
                if(station.isEmpty()){
                    player.sendMessage(ChatColor.RED+"Nombre de emisora inválido.");
                    return true;
                }
                if(!addon.available()){
                    player.sendMessage(ChatColor.RED+"La radio requiere Plasmo Voice activo en servidor y cliente.");
                    return true;
                }
                UUID uuid=player.getUniqueId();
                String name=player.getName();
                player.sendMessage(ChatColor.GOLD+"◉ Sintonizando "+args[1]+"…");
                try {
                    tasks.execute(() -> tune(player,uuid,name,station));
                } catch(java.util.concurrent.RejectedExecutionException ignored) {
                    player.sendMessage(ChatColor.RED+"VSRadio se está deteniendo.");
                }
            }
            case "apagar", "salir", "off" -> {
                UUID id=player.getUniqueId();
                tasks.execute(() -> {remove(id);tell(player,ChatColor.GRAY+"◉ Radio apagada.");});
            }
            case "estado" -> tasks.execute(() -> {
                String station=tuning.get(player.getUniqueId());
                if(station==null){tell(player,ChatColor.GRAY+"No estás sintonizando una emisora.");return;}
                try{
                    Now now=fetchNow(station);
                    tell(player,ChatColor.GOLD+"◉ "+now.displayName+" · "+now.track
                        +" · "+(now.positionMs/1000)+" s (en directo)");
                }catch(Exception error){tell(player,ChatColor.RED+"No se pudo consultar la emisora: "+error.getMessage());}
            });
            case "emisoras" -> tasks.execute(() -> {
                try {
                    JsonArray stations=requestJson("/api/v1/radio/stations").getAsJsonArray("stations");
                    List<String> names=new ArrayList<>();
                    for(var item:stations) names.add(item.getAsJsonObject().get("display_name").getAsString());
                    tell(player,ChatColor.GOLD+"Emisoras: "+(names.isEmpty()?"ninguna publicada":String.join(", ",names)));
                }catch(Exception error){tell(player,ChatColor.RED+"No se pudo consultar VSRadio.");}
            });
            default -> player.sendMessage(ChatColor.YELLOW
                +"Uso: /vsradio emisora <nombre> | emisoras | estado | apagar");
        }
        return true;
    }

    private void tune(Player player,UUID id,String username,String station) {
        try {
            Now now=fetchNow(station); // Reject unknown station before modifying the old tuning.
            if(addon.voiceServer().getPlayerManager().getPlayerByName(username)
                .filter(voice -> voice.getModVersion().isPresent()).isEmpty()) {
                tell(player,ChatColor.RED+"Activá y conectá Plasmo Voice en tu cliente para escuchar VSRadio.");
                return;
            }
            String previous=tuning.get(id);
            if(!station.equals(previous) && !broadcasts.containsKey(station)
                    && broadcasts.size()>=maxActiveStations) {
                tell(player,ChatColor.RED+"Capacidad de emisoras activas agotada ("+maxActiveStations+").");
                return;
            }
            if(previous!=null&&!previous.equals(station))remove(id);
            StationAudio audio=broadcasts.computeIfAbsent(station,StationAudio::new);
            audio.players.put(id,username);
            tuning.put(id,station);
            refresh(audio,now);
            tell(player,ChatColor.GREEN+"◉ "+now.displayName+" · sintonizada en "
                +(now.positionMs/1000)+" s de "+now.track+". /vsradio apagar para salir.");
        } catch(Exception error) {
            tell(player,ChatColor.RED+"No se pudo sintonizar "+station+": "+error.getMessage());
        }
    }

    private void remove(UUID id) {
        String station=tuning.remove(id);
        if(station==null)return;
        StationAudio audio=broadcasts.get(station);
        if(audio==null)return;
        audio.players.remove(id);
        if(audio.players.isEmpty()) {
            closeStream(audio);
            broadcasts.remove(station);
        }else updateListeners(audio);
    }

    void quit(UUID id) {
        try {tasks.execute(() -> remove(id));}
        catch(java.util.concurrent.RejectedExecutionException ignored) {}
    }

    private void tickSafely() {
        try {
            if(!addon.available())return;
            for(StationAudio audio:List.copyOf(broadcasts.values())){
                try {refresh(audio,fetchNow(audio.station));}
                catch(Exception error) {
                    long now=System.currentTimeMillis();
                    if(now-audio.lastWarning>30000){
                        audio.lastWarning=now;
                        logger.warning("VSRadio "+audio.station+": "+error.getClass().getSimpleName()
                            +": "+error.getMessage());
                    }
                }
            }
        }catch(RuntimeException error){logger.warning("VSRadio: "+error.getMessage());}
    }

    private void updateListeners(StationAudio audio) {
        if(audio.source==null)return;
        Set<VoicePlayer> group=new HashSet<>();
        for(String username:audio.players.values()) {
            addon.voiceServer().getPlayerManager().getPlayerByName(username)
                .ifPresent(group::add);
        }
        audio.source.setPlayers(group);
    }

    private void refresh(StationAudio audio,Now now) throws IOException,InterruptedException {
        if(audio.sender!=null && audio.trackStarted==now.trackStartedMs
            && audio.revision.equals(now.revision)&&audio.track.equals(now.track)) {
            updateListeners(audio);
            return;
        }
        String name=now.track;
        if(!name.matches("[a-z0-9_-]{1,48}"))throw new IOException("Pista inválida");
        byte[] pcm=getBytes("/api/v1/radio/tracks/"+name+"/pcm");
        if(pcm.length<1920||pcm.length>MAX_PCM_BYTES||pcm.length%2!=0){
            throw new IOException("PCM mono 48k inválido");
        }
        int samplesCount=pcm.length/2;
        int offset=sampleOffset(now.positionMs,now.serverUnixMs,
            System.currentTimeMillis(),samplesCount);
        if(offset>=samplesCount-960)return; // Next tick will reach next track.
        short[] samples=new short[samplesCount-offset];
        for(int i=0;i<samples.length;i++){
            int at=(offset+i)*2;
            samples[i]=(short)((pcm[at]&0xff)|((pcm[at+1]&0xff)<<8));
        }
        // Never silence the previous source until the next track is prepared.
        ServerBroadcastSource newSource=addon.line().createBroadcastSource(false);
        ArrayAudioFrameProvider provider=new ArrayAudioFrameProvider(addon.voiceServer(),false);
        AudioSender sender;
        try{
            newSource.setPlayers(new HashSet<>(audio.players.values().stream()
                .map(namePart -> addon.voiceServer().getPlayerManager().getPlayerByName(namePart))
                .filter(java.util.Optional::isPresent)
                .map(java.util.Optional::get).toList()));
            sender=newSource.createAudioSender(provider);
            sender.onStop(() -> {
                provider.close();
                newSource.remove();
            });
            provider.addSamples(samples);
            sender.start();
        }catch(RuntimeException error){
            provider.close();newSource.remove();throw error;
        }
        AudioSender previous=audio.sender;
        audio.source=newSource;
        audio.sender=sender;
        audio.trackStarted=now.trackStartedMs;
        audio.track=now.track;
        audio.revision=now.revision;
        if(previous!=null)previous.stop();
    }

    private void closeStream(StationAudio audio) {
        AudioSender sender=audio.sender;
        audio.sender=null;
        audio.source=null;
        audio.trackStarted=Long.MIN_VALUE;
        if(sender!=null)sender.stop();
    }

    private Now fetchNow(String station) throws IOException,InterruptedException {
        JsonObject data=requestJson("/api/v1/radio/stations/"+station+"/now");
        return new Now(station,data.get("display_name").getAsString(),
            data.get("track").getAsString(),data.get("revision").getAsString(),
            data.get("position_ms").getAsLong(),data.get("track_started_unix_ms").getAsLong(),
            data.get("server_unix_ms").getAsLong());
    }

    private JsonObject requestJson(String path) throws IOException,InterruptedException {
        byte[] data=getBytes(path);
        if(data.length>256*1024)throw new IOException("Respuesta JSON demasiado grande");
        return JsonParser.parseString(new String(data,StandardCharsets.UTF_8)).getAsJsonObject();
    }

    private byte[] getBytes(String path) throws IOException,InterruptedException {
        if(!path.startsWith("/api/v1/radio/")||path.contains("?")||path.contains("..")){
            throw new IOException("Ruta local de radio inválida");
        }
        HttpRequest request=HttpRequest.newBuilder(CORE.resolve(path))
            .GET().timeout(Duration.ofSeconds(8)).build();
        HttpResponse<byte[]> result=http.send(request,HttpResponse.BodyHandlers.ofByteArray());
        if(result.statusCode()!=200) {
            if(result.statusCode()==404)throw new IOException("Emisora o audio no publicados");
            throw new IOException("VSCore devolvió HTTP "+result.statusCode());
        }
        if(result.body().length>MAX_PCM_BYTES)throw new IOException("Respuesta de audio supera 32 MiB");
        return result.body();
    }

    @Override
    public void close() {
        // Execute cleanup on the SAME single worker as the station maps;
        // a Bukkit disable thread must not race with an in-flight HTTP tick.
        try {
            java.util.concurrent.Future<?> done=tasks.submit(() -> {
                for(StationAudio audio:broadcasts.values())closeStream(audio);
                broadcasts.clear();
                tuning.clear();
            });
            done.get(10,TimeUnit.SECONDS);
        }catch(java.util.concurrent.RejectedExecutionException ignored) {
            // Shutdown already started.
        }catch(Exception error) {
            logger.warning("VSRadio no pudo limpiar a tiempo: "
                +error.getClass().getSimpleName());
        }finally {
            tasks.shutdownNow();
        }
    }
}
