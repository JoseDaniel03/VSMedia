# Audio Studio — edición no destructiva (núcleo implementado)

## Qué se puede editar desde el modelo

El Audio Engine mantiene las estructuras anteriores `AudioProject`,
`AudioTrack` y `AudioClip`. Las modificaciones nuevas se expresan como
`ClipEdit` y `TrackEdit` sin modificar el audio fuente ni duplicar archivos.

| Ámbito | Control actual | Procesamiento |
|---|---|---|
| Clip | `source_in_ms` | Recorte desde un punto del medio original |
| Clip | `pan` | Balance estéreo de −1 a +1 |
| Clip | `fade_in_ms`, `fade_out_ms` | Envolventes lineales por muestra |
| Clip | `reversed`, `muted` | Reversa de un segmento o silencio por clip |
| Clip | `gain_points` | Automatización en tiempos ascendentes y ganancia dB |
| Clip | `filters` | Ecualizadores LowPass/HighPass/Peaking RBJ encadenados |
| Pista | `gain_db`, `pan`, `muted`, `solo` | Mezcla por pista, silenciar/solo y balance |

El render usa `render_project_advanced(project, sources, sample_rate,
clip_edits, track_edits)`. La función `render_project` anterior se conserva
y ofrece el mismo resultado cuando no hay ediciones. Los resamples de
un mismo archivo se cachean durante la operación. La salida mantiene
el límite de hasta cinco millones de frames.

## Guardado VSAudio

El archivo `.vsaudio` guarda `timeline`, `sources`, `clip_edits` y
`track_edits`. Los dos últimos son opcionales: los proyectos sin esos
campos se abren y serializan sin cambios de comportamiento. Todo ajuste
se valida antes de publicar el manifiesto, incluidos los parámetros EQ.

Ejemplo de configuración (fragmento de `payload.data`):

```json
{
  "clip_edits": [{
    "track_index": 0, "clip_index": 0, "source_in_ms": 250,
    "pan": -0.3, "fade_in_ms": 100, "fade_out_ms": 200,
    "reversed": false, "muted": false,
    "gain_points": [{"at_ms": 0, "gain_db": -12},
                    {"at_ms": 500, "gain_db": 0}],
    "filters": [{"type": "low_pass", "frequency_hz": 2500, "q": 0.707}]
  }],
  "track_edits": [{"track_index": 0, "gain_db": -3,
                   "pan": 0, "muted": false, "solo": false}]
}
```

## API de comandos para la interfaz

`vsmedia-audio::editing::AudioEditCommand` expone acciones serializables:
`upsert_clip`, `remove_clip`, `upsert_track` y `remove_track`. El editor
puede enviar un cambio sin reconstruir el proyecto completo.

`apply_edit_command()` prepara copias de las configuraciones, valida todos
los parámetros y solamente entonces confirma ambas listas. En caso de
error, el proyecto permanece sin cambios. En formatos nativos está
disponible `VSAudio::apply_edit(command)`.

Estas operaciones aún no realizan persistencia SQLite por sí solas;
el servicio de proyectos guardará una nueva revisión mediante el
`AssetRepository` después de aplicar los comandos.


## Identificadores estables y operaciones de línea de tiempo

El módulo `vsmedia-formats::audio_studio` incorpora `AudioStudio`, que trabaja
sobre un documento `.vsaudio` en memoria. Se pueden ejecutar comandos
`StudioCommand` para mover, insertar, borrar y reemplazar clips, además de
mover, crear, eliminar o renombrar pistas.

El documento conserva identidades `t-N` y `c-N` en su campo opcional
`identities`; los proyectos anteriores se leen sin cambios y obtienen esos
identificadores en su primera edición. Al reordenar, el motor actualiza de
forma coordinada los índices de los ajustes existentes.

`EditClipById` y `EditTrackById` permiten modificar parámetros utilizando
el ID en lugar de un índice, incluso después de reorganizar elementos.
Se rechazan referencias a IDs inexistentes y documentos con IDs duplicados
o desalineados respecto de la línea de tiempo.

`AudioStudio::apply(command)` prepara una copia del manifiesto, comprueba
las reglas y solo confirma el cambio si es válido. `undo()` y `redo()`
restauran **la línea de tiempo completa, los ajustes y los IDs**, sin copiar
los medios PCM. El historial mantiene un máximo global de 64 estados y 16 MiB
compartidos entre deshacer y rehacer. Un cambio nuevo borra el historial redo.
Este historial es temporal; todavía no se guarda en SQLite.

El módulo se puede usar desde la futura API del editor, pero todavía no
contiene interfaz web, selección visual ni arrastre de clips.

## Formas de onda y vista previa

`vsmedia-audio::analysis::waveform_peaks(frames,frames_per_bucket)`
resume PCM estéreo en bloques de mínimos/máximos/RMS, con tope de 8192
bloques de salida. Un futuro panel puede cambiar `frames_per_bucket`
para hacer zoom sin transferir todo el PCM. `spectrum()` aporta FFT/Hann
para el espectro de frecuencias y `rms()` para medidores.

## Pendiente: editor visual completo

- UI de línea de tiempo con arrastre, ajuste y preview audible inmediato.
- Persistir el historial entre sesiones y soportar edición simultánea con detección de conflictos.
- Edición de regiones, crossfades configurables, pitch/time-stretch,
  compresor, limitador, reverb, filtros avanzados y gestión de buses.
- Códecs comprimidos MP3/FLAC/OGG y exportación OGG nativa para Minecraft.
- Integración efectiva con el plugin Folia y VSClient para audio en vivo.
- Automatización con curvas alternativas a interpolación lineal y
  procesamiento de efectos fuera de línea por lotes.

Esta implementación es el motor de edición **por código fuente y formato**;
no representa un editor web ya disponible ni una versión final del plugin.

## FullCanvas — controles conectados y render Rust

El backend local Axum admite ahora `GET /api/v1/audio/projects/{id}/render`,
que recupera un `.vsaudio` almacenado, verifica los hashes SHA-256 de sus
fuentes, decodifica **WAV** con `vsmedia-audio`, procesa la línea de tiempo
con `render_project_advanced` y devuelve un WAV PCM16 (48 kHz estéreo).
La operación se ejecuta con `spawn_blocking` para no bloquear el executor
HTTP; los límites de tamaño son los mismos del Audio Engine.

Los botones **▶ Motor** y **↓ WAV** del catálogo VSAudio/VSMix llaman a este
endpoint para reproducir o descargar exactamente el resultado renderizado.
La reproducción del archivo original que ya existía en VSAudio sigue siendo
una escucha rápida sin procesamiento completo; no debe confundirse con la
previsualización Rust.

VSAudio ahora expone visualmente **paneo**, filtros pasa-altos y pasa-bajos
(`Q=0.707`) y **automatización lineal del volumen al final del clip**.
Estos valores se guardan en `clip_edits` y se recuperan al abrir un proyecto
anterior. No se modifican los archivos fuente.

El render nativo solo admite fuentes WAV por el momento. Los proyectos con
MP3/FLAC/OGG/Opus/WebM reciben HTTP 422 con explicación; sus archivos
pueden seguir almacenándose y reproduciéndose localmente. La integración
con el editor visual de múltiples regiones y el resto de los efectos
permanece pendiente.

## Regiones en el Studio web

Cada pista VSMix puede representar varios clips nativos, no solo un
archivo. Las regiones tienen ID local y controles separados de inicio,
duración, in-point, volumen, paneo y fades. Guardar la mezcla serializa
la posición/ganancia en el timeline nativo y los fades/paneo en
`clip_edits`; Abrir la mezcla reconstruye la lista completa de regiones.

La previsualización del proyecto actual utiliza
`POST /api/v1/audio/render` sin guardar una revisión del proyecto,
aunque las fuentes usadas se publican en el CAS local mediante acción
explícita. El render de Rust usa su procesamiento habitual y verifica
los SHA-256 antes de leer archivos. No hay soporte de OGG/Opus/MP3
nativo en esta ruta por ahora.

## FullCanvas VSMix: ondas locales y crossfades lineales (26.3)

Las regiones conservan sus identificadores y sus campos en proyectos .vsaudio.
El desplazamiento y los recortes editan únicamente la sesión local. La
previsualización exacta continúa utilizando Motor (borrador) de Rust.

- El botón Ondas analiza únicamente las fuentes actualmente utilizadas.
  Decodifica con Web Audio en el navegador, sin subir audio para generar picos.
  Conserva hasta 768 picos por fuente en memoria, con límite de 16 MiB por
  archivo y 10 millones de cuadros decodificados por fuente. Audio muy largo
  o códecs no compatibles pueden no tener gráfica.
- El botón Crossfade aplica entradas y salidas lineales a pares de regiones
  superpuestas en una misma pista. Omite superposiciones triples, anidadas
  o cuyos fades no caben dentro de la duración disponible. Reutiliza las
  propiedades fade_in y fade_out existentes del motor: no agrega nuevos
  campos al proyecto ni impone un nuevo algoritmo de mezcla.
- El editor de regiones lee los valores actuales de la sesión tras arrastres
  y transiciones, evitando números obsoletos en los campos de control.
- Las ondas son resúmenes para inspección visual, no la representación exacta
  del proceso DSP. La vista previa definitiva es Motor (borrador).

Queda pendiente la comprobación manual en navegador de los gestos pointer,
la alineación visual de las ondas con sus recortes y la interacción entre
regiones de distintas pistas. Los datapacks continúan fuera del alcance.

## VSMix Timeline: equal-power y envolventes editables (26.3)

El motor nativo añade a la edición de cada clip el campo opcional
`fade_curve`: `linear` (predeterminado) o `equal_power`.
Los documentos anteriores no incluyen este campo y conservan exactamente
su respuesta lineal. El esquema principal de `.vsaudio` permanece en versión 1.

- En `equal_power`, el fade de entrada aplica `sin(π·t/2)`; el de salida
  aplica `cos(π·t/2)` para `t` normalizado entre 0 y 1. Así, durante
  un cruce simétrico se cumple `g₁² + g₂² ≈ 1`. Esta ley conserva la potencia
  entre fuentes no correlacionadas; no garantiza volumen constante cuando
  ambas contienen la misma señal (puede haber un aumento audible).
- VSMix ofrece un selector de curva para el botón Crossfade y otro por región.
  Los nuevos marcadores de entrada y salida pueden arrastrarse y ajustan
  `fade_in` y `fade_out` sin modificar la fuente, manteniendo la suma de
  las duraciones dentro de la región.
- La curva de volumen aparece como una línea blanca sobre las ondas.
  Se representa con la misma ecuación normalizada del Audio Engine.
  Los campos numéricos siguen disponibles para ajustes exactos.
- El formulario nativo `compose_mix` exporta la curva de cada región en
  `clip_edits`, y `load_mix` recupera el valor. El render de borrador
  y el guardado utilizan la misma implementación DSP de Rust.
- Se mantienen los límites existentes de duración y de regiones, los
  controles de integridad del backend y la restricción de fuentes WAV
  para la previsualización mediante Audio Engine.

Pruebas automáticas: compatibilidad de documentos anteriores, potencia
normalizada, mezcla multipista con solapamiento, y render de borrador
que no crea una revisión guardada. Falta una prueba manual en navegador
de arrastres, zoom, superposición visual de curvas y comportamiento táctil.

## VSMix 26.3 — Automatización de volumen por región

Cada región ahora permite añadir, mover y eliminar puntos de volumen locales
mediante **＋ Punto**, los campos numéricos y los tiradores de color turquesa
sobre la forma de onda. **Restablecer** vuelve a ganancia constante. Los puntos
se almacenan como tiempo en segundos y ganancia lineal (0–200%) y son
independientes del volumen, paneo y fades de la región.

El editor representa una curva de hasta 61 segmentos; el render nativo
recibe la lista original de puntos en `clip_edits.gain_points` con tiempo
en milisegundos y ganancia en dB. **Rust interpola en amplitud lineal, no
en decibelios**, por lo que la curva visual y la señal comparten la misma
regla. Los archivos anteriores conservan `gain_points: []` y no cambian
su comportamiento. No se modifica la versión del formato `.vsaudio`.

Los puntos deben estar estrictamente ordenados, dentro de la duración
del clip y no superar 256 por región. El límite de la interfaz es 200%;
al abrir archivos nativos con automatizaciones más altas, VSMix rechaza la
sesión en lugar de alterar su volumen silenciosamente. Los fades
continúan aplicándose *además* de la automatización, y la ganancia de la
región y la pista permanecen multiplicativas. Al recortar el comienzo,
se interpola el valor de volumen en el nuevo punto de inicio para no
alterar el sonido del fragmento conservado.

Los borradores se renderizan con el endpoint de audio sin crear revisión
de proyecto. La acción puede subir fuentes al CAS local (como antes);
la mera edición de puntos o su visualización no realiza ninguna subida.
Se comprueba la secuencia de puntos antes de iniciar una subida.

**Pruebas automáticas:** interpolación de puntos en el core Rust, rechazo de
puntos repetidos, render de borrador con envolvente en Axum y ausencia de
proyecto guardado. **Por comprobar manualmente en navegador:** gestos de
arrastre (incluido el uso táctil), zoom, recorte con automatización y
sincronización de campos numéricos.

Seguimos sin generar JAR, ZIP finales ni releases. Los datapacks quedan
fuera de esta fase.
