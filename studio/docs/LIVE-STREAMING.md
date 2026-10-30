# VSMedia Studio 26.3 — VSRadio y VSStreams

## Dos sistemas diferentes

- **VSRadio:** emisión local WebRTC P2P desde el navegador. No requiere
  Minecraft para probar playlists, música y locución en una sala pequeña.
- **VSStreams:** interfaz de integración de **Plasmo Voice**, el mod/plugin
  gratuito usado por Virgoland para el **chat de voz por proximidad dentro
  de Minecraft**. No crea salas de voz alternativas ni pide micrófono.
  La integración administrativa con VSEngine se realizará después.

## VSRadio: funciones implementadas

- Playlists persistentes en **localStorage** (hasta 32 listas de 512 entradas).
  Se guardan nombres de archivo, orden e identificadores, **no los bytes
  del audio**. Tras recargar se deben reimportar las fuentes para
  escucharlas: VSRadio reencuentra automáticamente un archivo con el
  mismo nombre si cambió su identificador temporal. Si existen varios
  archivos con nombres idénticos se utiliza la primera coincidencia,
  por lo que conviene darles nombres únicos.
- Reordenar pistas, eliminar, grabar voz local, reproducción secuencial,
  anterior/siguiente, omitir archivos ausentes y repetición configurable.
- Programación como notas locales, no como scheduler de emisión en backend.
- Radio en vivo: un **emisor** y hasta **15 oyentes** en una sala WebRTC.
  Fuentes de transmisión: micrófono, reproductor local de la playlist o
  mezcla de ambas con controles separados de ganancia de música/micrófono.
- Código aleatorio de sala que se puede copiar; mute local del micrófono,
  salir, métricas por conexión y mensajes de estado para diferenciar
  conexión a señalización de una conexión real de audio.
- Rust/Axum sólo **señaliza** SDP/ICE; la voz y la música viajan entre
  navegadores mediante WebRTC cifrado. Axum no graba, decodifica ni
  almacena el audio de las salas.
- Heartbeat cada 30 segundos y desconexión por 100 segundos de inactividad.
  Máximo 16 peers por sala y 128 conexiones por instancia; los códigos
  de sala son identificadores, **no contraseñas**.

### Probar la radio localmente

En `studio/` ejecutar `./start.sh`. Abrir
`http://127.0.0.1:3060` en dos pestañas del mismo equipo con auriculares.
En la primera, entrar a VSRadio, cargar una fuente en la biblioteca,
agregarla a una playlist, reproducirla y crear código de sala. Elegir
**Emisor**, fuente **Playlist**, luego **Conectar**. En la segunda pestaña,
ingresar el mismo código, elegir **Oyente** y conectar. La conexión real
de audio debe mostrarse mediante el estado de WebRTC, no sólo por
haber ingresado a la sala.

**Límite esencial:** el transmisor P2P genera una copia de la señal por
oyente. Esto no es una radio de difusión pública escalable ni una
emisión conectada al mundo Minecraft. Para ello se requerirá un
SFU o distribución segmentada, infraestructura TLS/WSS, autenticación y
servidores TURN en redes que lo necesiten. Por defecto no se consulta
ningún STUN/TURN externo. La URL WS actualmente apunta a loopback.

## VSStreams y Plasmo Voice

Plasmo Voice implementa el micrófono, audio posicional 3D, distancia,
Push-to-Talk, permisos y muteos dentro de Minecraft. Se instala en
**VSEngine/CanvasMC (servidor Folia/Paper)** y como mod Fabric en
**VSClient**. Ya existen funciones administrativas nativas, como
`/vmute`, `/vunmute` y `/vlist`; no vamos a reimplementarlas como
otro chat de voz.

La sección VSStreams informa el estado real mediante
`GET /api/v1/streams/integration`. **Ahora devuelve
`server_connected: false`**, porque el puente Axum no dispone de un
adaptador autenticado a VSEngine/Plasmo. La página presenta instrucciones
de instalación verificables y evita inventar jugadores, mutear desde
la web o simular conexiones. La futura consola de moderación (muteos,
historial, permisos, políticas por mundo y regiones) queda expresamente
pospuesta.

Las URLs de descarga oficiales son
https://modrinth.com/plugin/plasmo-voice y
https://modrinth.com/plugin/plasmo-voice/versions?l=fabric .
La API oficial para futuros adaptadores está en
https://plasmovoice.com/docs/api/ .

## Seguridad y verificaciones

El bridge sigue limitado a `127.0.0.1:3061`, valida el Origin local
para conexiones WebSocket y no debe exponerse a Internet sin autorización.
`/api/v1/live/ws` **sólo acepta `mode: radio`** y rechaza el modo
anterior `voice`. No hay almacenamiento del audio de la conversación.

GitHub Actions comprueba Rust, Axum, Leptos wasm32, Java, las pruebas
de navegación de playlists y `node --check` del cliente WebRTC.
**No sustituye** la prueba E2E en navegadores reales con micrófonos, ni
verifica que Plasmo esté instalado en el servidor de Minecraft.

Sin JAR, ZIP finales ni releases. Los datapacks y la administración
avanzada de Plasmo se abordarán en otra etapa.
