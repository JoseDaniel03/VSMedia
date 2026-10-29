# Conexión VSMedia Studio → VSCore (fase de desarrollo)

Esta web se recuperó del ZIP original y conserva el diseño FullCanvas de Rust/Leptos, biblioteca local, VSAudio, VSMix y VSRadio.

## Ahora sí conectado
- `backend` es Axum sobre `127.0.0.1:3061`, enlazado al `SqliteAssetStore` **real** de `crates/vsmedia-core`.
- `POST /api/v1/assets/{id}` publica audio en namespace `studio` con control transaccional ACL y SHA-256.
- `GET /api/v1/assets/{id}` lee el contenido publicado.
- `POST /api/v1/audio/projects/{id}` valida/guarda proyectos nativos `.vsaudio` con revisiones.
- `GET /api/v1/audio/projects/{id}` abre documentos nativos almacenados.
- `POST /api/v1/audio/projects/{id}/commands` ejecuta `StudioCommand` con el editor Rust existente. Requiere `expected_sha256` actual (comprobación optimista, **aún no es compare-and-swap atómico**).
- La interfaz incorpora **Guardar proyecto** en VSAudio: publica el archivo original y guarda la versión como `.vsaudio` no destructivo. Recuerda establecer la duración final o dejar cargar los metadatos del audio.

No se simula emisión de radio ni streaming; los endpoints sin implementar responden 501. Audio de VSMix/Radio sigue siendo local del navegador por ahora.

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
- El guardado de proyecto guarda una sola versión y sus parámetros actuales; el futuro editor multipista enlazará la línea de tiempo completa y `AudioStudio` de Rust.

## Comandos del editor
El documento de proyecto se puede modificar mediante:
```http
POST /api/v1/audio/projects/{id}/commands
Content-Type: application/json

{"expected_sha256":"<hash actual>","command":{"operation":"rename_track","index":0,"name":"Voz"}}
```
El hash se devuelve cuando se guarda el proyecto o se lee su `AssetRecord`. La comprobación es optimista; para colaboración remota hará falta una operación de publicación atómica con revisión esperada.
