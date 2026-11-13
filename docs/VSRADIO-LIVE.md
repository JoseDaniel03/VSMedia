# VSRadio sincronizada con Minecraft (CanvasMC/Folia 26.3)

## Uso del jugador

~~~text
/vsradio emisora LatinSabroso
/vsradio emisoras
/vsradio estado
/vsradio apagar
~~~

El nombre de la emisora se normaliza sin distinguir mayúsculas ni tildes:
"LatinSabroso" -> "latinsabroso". El permiso \`vsradio.listen\` es público
por defecto. No se necesita un resource pack para escuchar el stream de radio
porque **la música se envía mediante Plasmo Voice**, no con el packet de sonido
de Minecraft. Sí se requiere tener **Plasmo Voice instalado y conectado en el
cliente** (Fabric) y en el servidor.

## Publicar una emisora desde la web

1. Abrir la **web original VSMedia** y entrar en **VSRadio**.
2. Crear una playlist con el nombre deseado, por ejemplo \`LatinSabroso\`.
3. Importar las canciones en la biblioteca y añadirlas a esa playlist.
4. Si la playlist ya suena localmente, dejar que reproduzca. El botón
   **◉ Emitir en Minecraft** captura la canción y el segundo actuales.
5. Presionar **◉ Emitir en Minecraft**. El navegador decodifica cada archivo,
   convierte a PCM16 mono a 48 kHz en un AudioContext/OfflineAudioContext,
   y lo sube al almacenamiento SQLite/CAS de VSCore. Se publica también una
   programación persistente con reloj UTC.
6. El servidor lee la programación, y cuando un jugador ejecuta
   \`/vsradio emisora LatinSabroso\`, se incorpora a la fuente de
   difusión Plasmo Voice correspondiente. Comparte **la posición que ya
   escuchan los oyentes**, sin reiniciar el tema.
7. El monitor de VSRadio en la web ajusta su reproductor cada 3 segundos al
   reloj del servidor. La transición entre canciones se verifica por el
   proceso de audio cada 250 ms, de forma aproximada.

La reproducción de una emisora **continúa su reloj global mientras no hay
oyentes**: al sintonizar más tarde, se busca la canción que corresponde al
tiempo transcurrido desde el inicio de la programación, incluso después de un
reinicio del puente Rust, siempre que el catálogo SQLite se conserve.

## Modo DJ en directo (micrófono o playlist + micrófono)

La web FullCanvas original tiene ahora botones adicionales, sin reemplazar
VSRadio. Primero crear y emitir la playlist desde «◉ Emitir en Minecraft».
A continuación elegir **Micrófono DJ** o **Playlist + DJ** y pulsar
**◉ Abrir DJ**. El navegador solicita permiso al micrófono y mezcla a
48 kHz mono; **no almacena la voz**.

- **Micrófono DJ:** la radio transmite solo la voz del operador. La
  programación de la playlist sigue teniendo un reloj compartido, y
  vuelve a oírse al cerrar el DJ.
- **Playlist + DJ:** la radio transmite música y locución juntas.
  Los deslizadores Música, DJ y Ducking permiten equilibrar niveles.
  Ducking atenúa la música cuando detecta voz y la recupera lentamente
  tras una pausa en la locución.
- **Silenciar** pone el nivel del DJ a cero sin desconectar a los oyentes;
  **■ Cortar DJ** termina la sesión y devuelve a todos los oyentes a
  la playlist compartida.
- Los controles **Ⅱ Pausar emisión**, **▶ Reanudar**, **⏮ Anterior**,
  **⏭ Siguiente** y **⏱ Aplicar posición** editan el reloj de la emisora
  persistido en VSCore. Esto afecta a todos los oyentes, no solo a la
  vista local.

La captura usa AudioWorklet preferentemente y ScriptProcessor como fallback.
En ambos casos forma lotes de **200 ms (10 bloques de 20 ms)** de PCM16
mono/48k, sin generar archivos en el servidor. El puente Rust conserva
únicamente una cola temporal acotada; Java lee los bloques nuevos y los
entrega al proveedor de cola de Plasmo Voice. Si el operador cierra la
pestaña o pierde la conexión durante más de 3 segundos, la sesión
caduca y el servidor recupera automáticamente la playlist.

**Importante:** la radio para jugadores de Minecraft depende del mod
Plasmo Voice en cada cliente. El modo P2P WebRTC de la web es independiente
y no es el transporte de la radio dentro del juego.

## Diseño técnico

- \`studio/assets/radio-sync.js\`: mejora aditiva del editor existente,
  sin reemplazar menús ni paneles, usa los archivos de la playlist activa
  sin tener que volver a seleccionarlos.
- \`studio/backend/src/radio.rs\`: endpoints persistentes en CAS que
  comprueban el formato WAV y calculan posición/tema desde \`anchor_unix_ms\`.
- \`RadioVoiceAddon.java\`: inicializa una línea de fuente Plasmo Voice
  \`vsradio\`, utilizando su API oficial.
- \`RadioService.java\`: registra sintonizadores de jugadores, compara
  el tema y posición con VSCore, y envía el resto de la pista desde el punto
  correspondiente a un **broadcast source único por emisora**. Los
  segundos oyentes se suman al mismo broadcaster, sin descargar PCM por
  jugador. La codificación de los frames Plasmo se realiza con
  \`ArrayAudioFrameProvider\`, fuera de los hilos de Folia.
- \`VSMediaPlugin.java\`: carga el addon en \`onLoad\`, registra el comando y
  limpia la membresía al salir del servidor.
- El navegador convierte las pistas a **48 kHz, mono y PCM de 16 bits**.
  Plasmo transmite frames de 20 ms. No se intenta usar \`playSound\` ni
  simular un seek de un sonido de resource pack.

El sistema DJ agrega POST /stations/{id}/live/start, POST /live/push
(con token efímero x-vsradio-session), POST /live/stop, GET /live/status y
GET /live/drain/{seq}. La cola está limitada a 24 lotes de hasta 200 ms
por emisora, con máximo 8 sesiones DJ. Solo el publicador que obtuvo el
token puede empujar o detener su sesión; expira sin audio. Java recibe
solo PCM por localhost; los usuarios remotos no acceden a estos endpoints.

### Endpoints VSCore (solo localhost)

| Método | Ruta | Uso |
|---|---|---|
| POST | /api/v1/radio/tracks/{id} | Subir un WAV PCM16 mono 48 kHz |
| GET | /api/v1/radio/tracks/{id}/pcm | Pista PCM para el addon local |
| POST | /api/v1/radio/stations/{id} | Registrar playlist, nombre e inicio |
| GET | /api/v1/radio/stations/{id}/now | Emisora, tema y segundo en vivo |
| GET | /api/v1/radio/stations | Emisoras disponibles |

La publicación usa la misma limitación por origen de la API local; no exponer
puertos 3060/3061 a Internet. Los archivos PCM nunca se sirven directamente
al navegador de un jugador: se envían por la red de voz instalada.

## Límites y seguridad

- Máximo 32 emisoras registradas; hasta 8 sintonizadas al mismo tiempo;
  128 archivos de radio, 64 pistas por playlist, 384 MiB de PCM persistente
  y 32 MiB por archivo.
- El editor limita cada pista a 340 segundos para controlar el costo de
  decodificación y almacenamiento.
- Al cambiar de emisora, la fuente anterior deja de tener a ese oyente.
  \`/vsradio apagar\` también lo elimina.
- Si la estación no existe, Plasmo Voice no está conectado o VSCore
  no responde, el comando informa un error en lugar de reproducir
  música incorrecta.
- Cuando se termina un tema, la transición usa un monitor periódico;
  el reloj es compartido, pero pueden existir pequeñas demoras de
  red/decodificación. No se promete sincronía sin error de milisegundos.
- **Playlist y DJ en directo** ya tienen ruta de código: web → Rust →
  Plasmo Voice. Quedan pendientes la validación manual en navegadores
  reales, la estabilidad de la cola de Plasmo bajo carga y la comprobación
  de escucha en al menos dos clientes Minecraft. No se afirma sincronía
  exacta de milisegundos.
- El addon depende del plugin \`PlasmoVoice\` y de su mod cliente compatible.
  Al cambiar de versión de Plasmo Voice, revisar compatibilidad de API.

## Estado de entrega

Esta tanda es **código fuente + pruebas**, sin generar ni subir JARs:
el job CI de empaquetado sigue deshabilitado por solicitud del usuario.
Para disponer del comando dentro de Minecraft será necesario compilar e
instalar en una etapa posterior el plugin actualizado, con Plasmo Voice
instalado en el servidor. La prueba real de sonido entre dos clientes
Minecraft queda pendiente hasta ese despliegue.
