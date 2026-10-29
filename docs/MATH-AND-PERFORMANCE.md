# Matemáticas y rendimiento: criterios verificables

No añadir complejidad matemática salvo que resuelva un problema comprobable. Preferir funciones
compartidas y librerías probadas. Las operaciones DSP deben ser in-place cuando sea viable.

## 1. Biquad EQ — audio

`vsmedia-audio/src/filter.rs` usa la forma directa II transpuesta con estados independientes
para cada canal. La ecuación normalizada es:

```
y[n]  = b0*x[n] + s1[n-1]
s1[n] = b1*x[n] - a1*y[n] + s2[n-1]
s2[n] = b2*x[n] - a2*y[n]
```

El conjunto RBJ se calcula en f64 y normaliza por a0; luego se ejecuta sobre PCM f32.
Los estados tienen reset para evitar memoria de filtro cruzada entre clips. Test: respuesta
a señal DC de low-pass/high-pass, identidad de peaking a 0 dB y parámetros inválidos.
No se reservan nuevos buffers en `StereoBiquad::process`.

## 2. FFT para medición y editor

`vsmedia-audio/src/analysis.rs` usa `rustfft` y ventana Hann:
`w[n] = 0.5 - 0.5*cos(2*pi*n/(N-1))`. El resultado es espectro de amplitud unilateral,
con normalización aproximada de ventana y bins en Hz. Test: pico de seno de 1 kHz.
Las FFT se calculan **offline**, no sobre hilos de región. Para pintar la línea de
tiempo, `waveform_peaks` produce por bucket mínimos, máximos y RMS estéreo
con máximo 8192 buckets, sin enviar PCM completo a la futura interfaz.

## 3. Spatial Audio

`vsmedia-audio/src/spatial.rs` calcula distancia Euclidiana 3D y atenuación de distancia
inversa, con caída suave al llegar al máximo para que el audio no se corte bruscamente.
La orientación del oyente determina paneo izquierda/derecha y la ley de potencia
constante crea niveles `[cos(theta), sin(theta)]`.
`dsp::equal_power_gains` es **la única implementación** del cálculo trigonométrico,
reutilizada en crossfade y posicionamiento espacial.

Son modelos básicos de espacialización; HRTF, oclusión, reverberación ambiental y
network Voice Chat todavía no están implementados.

## 4. Image Engine

`image::imageops::resize` y Lanczos3 generan un raster de alta calidad para fotos.
El método nearest existente se conserva para pixel-art y texturas, donde el filtrado
podría difuminar bordes. Las dimensiones y memoria de salida se validan antes de
comenzar la operación. Luego ambos usan el mismo mosaico `to_map_tiles`.

La decodificación estática PNG/JPEG/WebP/GIF usa ImageReader con límites; los fotogramas GIF/APNG se decodifican bajo cuotas de bytes y duración. No está implementada aún la paleta del mapa nativo ni colocación en MapView.

## 5. Remuestreo y mezclas multipista

`vsmedia-audio/src/resample.rs` utiliza Rubato 5 con `Fft<f32>` y
`FixedSync::Both`. `process_all()` evita el desplazamiento temporal inicial
del resampler e incluye el final de la señal. El remuestreo se limita a cinco
millones de fotogramas y se reutiliza dentro de cada render por asset/frecuencia.

`vsmedia-audio/src/codec.rs` importa WAV mono/estéreo PCM o float32,
expone el PCM estéreo validado y exporta WAV PCM16 cuantizado. No atribuirle
decodificación de MP3, FLAC u OGG, aún pendiente.

`vsmedia-audio/src/timeline.rs` reutiliza `dsp::mix_stereo`, aplica la
ganancia en dB al clip, omite pistas muteadas, cachea una sola versión
remuestreada por asset y limita tanto el largo final como la memoria combinada
de capas. El módulo `editing.rs` añade inicio de fuente, inversión,
automatización lineal por punto en amplitud, fades, EQ y control por pista.
`render_project_advanced` utiliza estos datos sin mutar los PCM de origen.
Aún no exporta OGG ni hace time-stretching con preservación de tono.

## 6. Pack Engine

`merge_structured` agrupa contribuciones por ruta y trata
`assets/<namespace>/sounds.json` como esquema de eventos: cada clave se resuelve por
prioridad de productor; claves distintas coexisten. Los datos JSON se serializan en
orden determinista; se rechazan colisiones de igual prioridad y se acota el tamaño.
`merge` se conserva como política única para binarios y esquemas desconocidos.

## 7. Evaluación

- Límite de memoria antes de reservar buffers grandes; validar NaN/infinito.
- No realocar ni decodificar por muestra en streaming; procesar por bloques.
- Antes de optimizar: tests de exactitud, benchmarks de CPU, uso de memoria y carga.
- Evitar crates internas para una sola función aislada: se generaliza cuando aparecen
  varios consumidores reales o una abstracción útil.
- No publicar artefactos finales hasta terminar el proyecto.
