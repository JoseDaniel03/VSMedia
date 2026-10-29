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

## Formas de onda y vista previa

`vsmedia-audio::analysis::waveform_peaks(frames,frames_per_bucket)`
resume PCM estéreo en bloques de mínimos/máximos/RMS, con tope de 8192
bloques de salida. Un futuro panel puede cambiar `frames_per_bucket`
para hacer zoom sin transferir todo el PCM. `spectrum()` aporta FFT/Hann
para el espectro de frecuencias y `rms()` para medidores.

## Pendiente: editor visual completo

- UI de línea de tiempo con arrastre, ajuste y preview audible inmediato.
- IDs persistentes de clips/pistas y operaciones de edición undo/redo.
- Edición de regiones, crossfades configurables, pitch/time-stretch,
  compresor, limitador, reverb, filtros avanzados y gestión de buses.
- Códecs comprimidos MP3/FLAC/OGG y exportación OGG nativa para Minecraft.
- Integración efectiva con el plugin Folia y VSClient para audio en vivo.
- Automatización con curvas alternativas a interpolación lineal y
  procesamiento de efectos fuera de línea por lotes.

Esta implementación es el motor de edición **por código fuente y formato**;
no representa un editor web ya disponible ni una versión final del plugin.
