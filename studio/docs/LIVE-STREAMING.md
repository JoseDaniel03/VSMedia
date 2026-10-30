# VSMedia 26.3 — VSRadio Live y VSStreams (primera integración)

## Alcance real

La consola ofrece **audio WebRTC de navegador a navegador** en salas pequeñas.
El backend Axum es únicamente servidor de **señalización WebSocket**:
transmite ofertas, respuestas y candidatos ICE; **no recibe, almacena,
graba ni mezcla audio**. La señal de voz viaja directamente por las
conexiones WebRTC cifradas entre participantes.

### VSRadio

En la pestaña **VSRadio**, abajo de las listas, aparece **Transmisión en
vivo**. Crear un código de sala alfanumérico, seleccionar **Emisor** y una
fuente (**Micrófono**, **Playlist** o **Playlist + micrófono**). Para playlist,
elegir primero un audio y reproducirlo en el reproductor de VSRadio. Al
pulsar **Conectar**, se abre la sala y cada oyente conectará su audio P2P.
La salida de la playlist se captura con Web Audio, se envía al destino
WebRTC sin copiar el archivo al backend y sigue sonando localmente.

Quienes escuchen eligen **Oyente**, ingresan el mismo código y pulsan
Conectar. No se solicita permiso de micrófono a los oyentes. Sólo hay
**un emisor por sala**, con un máximo de **16 participantes** contando
al emisor. El backend rechaza una segunda conexión como emisor.

### VSStreams / Salas de voz

La pestaña **VSStreams** permite crear o unirse a una sala de hasta
**8 participantes**. El usuario pulsa **Conectar**, concede permiso de
micrófono al navegador si acepta y puede silenciarlo/desilenciarlo o
salir. Cada otro participante se recibe a través de elementos HTMLAudio
con controles por si el navegador bloquea la reproducción automática.

### Arquitectura y transporte

- Rust/Axum: `GET /api/v1/live/ws`, WebSocket JSON, local
  `127.0.0.1:3061`; `GET /api/v1/live/status` indica conteos globales
  sin divulgar códigos de sala.
- Modo `radio`: rol `host` sólo puede ofrecer SDP; rol `listener`
  sólo puede responder. ICE únicamente entre emisor y oyente.
- Modo `voice`: rol `speaker` y malla de WebRTC entre participantes.
- Señales: `join` con `room`, `mode` y `role`; eventos
  `joined`, `peer_joined`, `peer_left`, `error`; `signal`
  incluye `to`, `kind` (`offer`, `answer`, `candidate`) y
  `data`. El servidor responde con `from`.
- Los mensajes JSON tienen un límite de 48 KiB, las colas tienen
  límite de 64, y hasta 128 participantes pueden conectarse a la vez.
  Las salas se eliminan de memoria cuando salen sus miembros.
- El audio de micrófono sólo se captura tras una acción explícita. Salir
  detiene las pistas de micrófono y cierra conexiones P2P. No existen
  grabaciones automáticas, subida de voz al CAS ni historial de chat.
- Protocolo de audio y compresión negociados por WebRTC, normalmente
  Opus. No se requiere ni inventa un formato propio para paquetes RTP.

## Limitaciones críticas

1. **Es un prototipo funcional local**, no una radio pública escalable ni
   un servicio remoto autenticado. El servidor escucha sólo en localhost.
   Los códigos de sala **no son contraseñas** ni autentican identidades.
2. El modo radio usa P2P: el emisor envía una copia de su stream por cada
   oyente. Para cientos/miles de oyentes se necesita un **SFU**,
   retransmisión/segmentación (por ejemplo HLS/LL-HLS) y control de ancho
   de banda y acceso.
3. No está configurado un servicio TURN. Por defecto `iceServers`
   está vacío para evitar contactar servicios externos sin consentimiento.
   Puede configurarse opcionalmente `window.VSMEDIA_ICE_SERVERS` en el
   navegador antes de cargar `live.js` si existe infraestructura propia.
   Dos redes/NAT distintos suelen requerir TURN.
4. **Internet:** se necesitan TLS/HTTPS, WSS, un reverse proxy con
   autenticación/roles/sesiones, control de invitados, rate limiting
   persistente y configuración explícita del endpoint WebSocket (el
   prototipo utiliza `ws://127.0.0.1:3061`). No exponer tal como está.
5. Web Audio `MediaElementAudioSourceNode` depende del soporte del
   navegador y captura el audio que pasa por el reproductor local. No
   se compone una mezcla multicanal de producción ni una programación
   automática de radio a nivel servidor.
6. Quedan pendientes pruebas manuales E2E con **dos o más navegadores**,
   permisos de micrófono, autoplay, reconexión, desconexión, NAT y
   efectos de audio del sistema. Los tests de Axum no prueban calidad
   acústica real. La emisión remota y el chat de voz en Minecraft no
   están conectados al plugin Java.

## Ejecución local

Desde `studio/`, ejecutar `./start.sh`: levanta Axum en
`127.0.0.1:3061` y Trunk en `127.0.0.1:3060`. Abrir la web, entrar
a VSRadio o VSStreams y crear/unirse a una sala. Para probar con dos
pestañas locales, mantener la misma sala y utilizar auriculares para
evitar realimentación.

Se mantiene la política del proyecto: **sin JAR, ZIP ni release
prematuro**. La integración a plugins/datapacks queda para la última
etapa de Virgoland.
