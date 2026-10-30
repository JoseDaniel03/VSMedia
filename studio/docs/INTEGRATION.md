# Conexión VSMedia Studio → VSCore (fase de desarrollo)

Esta web se recuperó del ZIP original y conserva el diseño FullCanvas de Rust/Leptos, biblioteca local, VSAudio, VSMix y VSRadio.

## Ahora sí conectado
- `backend` es Axum sobre `127.0.0.1:3061`, enlazado al `SqliteAssetStore` **real** de `crates/vsmedia-core`.
- `POST /api/v1/assets/{id}` publica audio en namespace `studio` con control transaccional ACL y SHA-256.
- `GET /api/v1/assets/{id}` lee el contenido publicado.
- `POST /api/v1/audio/projects/{id}` valida/guarda proyectos nativos `.vsaudio` con revisiones.
- `GET /api/v1/audio/projects` enumera hasta 256 proyectos guardados de Studio.
- `GET /api/v1/audio/projects/{id}` abre documentos nativos almacenados.
- `GET /api/v1/audio/projects/{id}/metadata` proporciona SHA-256 y revisión de un proyecto.
- `GET /api/v1/assets/{id}/metadata` permite comprobar la versión del archivo fuente antes de recuperarlo.
- `POST /api/v1/audio/projects/{id}/commands` ejecuta `StudioCommand` con el editor Rust existente. Requiere `expected_sha256` actual; la comprobación y escritura son **atómicas dentro de SQLite IMMEDIATE**.
- VSAudio incluye **Guardar proyecto** y **Proyectos guardados**: permite publicar una versión y recuperar un clip original con comprobación de SHA-256. Los proyectos de varios clips o pistas deben abrirse desde el futuro editor de regiones.
- VSMix incluye **Guardar mezcla** y **Abrir mezcla**: publica las fuentes utilizadas en las pistas, sus volúmenes, mute, solo y fades. Puede restaurar sesiones con hasta un clip por pista.

No se simula emisión de radio ni streaming; los endpoints sin implementar responden 501. Las operaciones de reproducción y grabación de VSMix/VSRadio siguen siendo locales del navegador, incluso aunque se guarden proyectos VSMix.

## Desarrollo local en CachyOS
Con Rust, `rustup target add wasm32-unknown-unknown` y `trunk` instalados:

Terminal 1 desde la raíz del repositorio:
```sh
cargo run --manifest-path studio/backend/Cargo.toml
```

Terminal 2:
```sh
cd studio
trunk serve --address 127.0.0.1 --port 3060
```

Abrir `http://127.0.0.1:3060` en el mismo equipo que ejecuta VSMedia. La base SQLite y el CAS se guardan en `./.vsmedia-data` relativo al directorio de ejecución del backend; puede definirse `VSMEDIA_DATA_DIR`.

## Límites/seguridad
- Solo escucha en loopback, y CORS se limita a `localhost:3060` y `127.0.0.1:3060`.
- Los endpoints de escritura validan `Origin` cuando está presente. Es un servicio **local de desarrollo, sin login multiusuario**; no publicar el puerto 3061 a Internet.
- Subidas individuales de hasta 16 MiB mediante HTTP y un límite de 2 MiB por proyecto nativo.
- No se incluyen artefactos de producción, plugin Jar ni releases.
- El bridge no emite radio ni sincroniza automáticamente todos los archivos/ediciones que permanecen en el navegador.
- VSAudio guarda una versión por operación; VSMix guarda una sesión con múltiples regiones por pista, con controles independientes de volumen, fuente, paneo, posición y fades. Faltan el arrastre visual, buses y efectos avanzados adicionales.
- **Abrir** recupera una copia de trabajo al navegador; **Guardar** crea otro proyecto, no actualiza silenciosamente el anterior.
- Los archivos publicados anteriormente permanecen en CAS; aún falta eliminación/GC y sincronización persistente de la biblioteca y carpetas.

## Comandos del editor
El documento de proyecto se puede modificar mediante:
```http
POST /api/v1/audio/projects/{id}/commands
Content-Type: application/json

{"expected_sha256":"<hash actual>","command":{"operation":"rename_track","index":0,"name":"Voz"}}
```
El hash se devuelve cuando se guarda el proyecto o se lee su `AssetRecord`. La comparación contra el SHA-256 esperado se realiza en la misma transacción de escritura SQLite que publica la nueva revisión; dos editores no pueden pisarse silenciosamente. No hay edición colaborativa en vivo ni merge automático.

## Render Rust conectado a la interfaz

`GET /api/v1/audio/projects/{id}/render` genera un WAV PCM16 de 48 kHz a
partir de un documento `.vsaudio` persistido. Usa el motor real de mezcla,
EQ, filtros, volumen, paneo, fades, reversa y automatización, verificando
previamente SHA-256 de los audios. Solo se admiten fuentes WAV: otros
códecs devuelven HTTP 422 sin simular una conversión.

Desde el catálogo de proyectos de VSAudio o VSMix, **▶ Motor** permite
previsualizar el resultado de Rust y **↓ WAV** lo prepara para descargar.
El panel VSAudio incorporó controles para paneo (-100% a +100%), filtros
pasa-altos/pasa-bajos (20–18000 Hz, si son válidos para la frecuencia de
muestreo) y ganancia final opcional. Esos ajustes se guardan/recuperan en
`clip_edits`; la escucha rápida del original no procesa estos controles.

El servicio sigue siendo solo local (127.0.0.1), sin publicación pública,
streaming ni integración Minecraft activa. No hay render comprimido.

## Editor de regiones multipista y render temporal

VSMix admite ahora múltiples regiones por pista. Cada región almacena
su archivo fuente, posición dentro de la pista, duración, in-point del
origen, volumen, paneo y fades. Se mantienen compatibles los proyectos
anteriores de una fuente por pista; el motor nativo permite un máximo
global de 256 clips por render y la UI limita la creación a 32 por pista.

`POST /api/v1/audio/render` recibe un manifiesto `.vsaudio` **sin
guardarlo como proyecto**, comprueba las referencias SHA-256 contra el CAS
y devuelve el WAV estéreo procesado con el Rust Audio Engine. VSMix lo
expone como **▶ Motor (borrador)** junto al reproductor del render.

Al pulsar ese botón, el frontend puede subir al CAS local los WAV fuentes
que todavía no están en VSCore; **no se crea revisión .vsaudio**. Este
comportamiento es explícito para distinguirlo de la reproducción local
HTML5. El botón clásico ▶ Reproducir se reserva al mezclador antiguo de
una fuente por pista: con regiones activas muestra un aviso para usar
el motor Rust.

El editor de regiones trabaja con datos en memoria del navegador. La
reproducción nativa está limitada a archivos WAV por ahora; las demás
fuentes pueden conservarse en el catálogo y como proyectos nativos,
pero no se simula render para códecs sin decodificador.

## Render temporal desde VSAudio

VSAudio también muestra **▶ Motor (borrador)** junto a su editor. La
versión actual (recortes, fades, reversa, paneo, EQ y volumen automatizado)
se procesa por Rust mediante el mismo endpoint temporal que VSMix.
Esto evita guardar el proyecto solo para escucharlo. La subida de la fuente
al CAS local ocurre exclusivamente cuando el usuario solicita el render.

El backend evita crear revisiones nuevas de una fuente existente si el
contenido recibido ya tiene el mismo SHA-256, reduciendo operaciones
innecesarias durante las previsualizaciones sucesivas.
