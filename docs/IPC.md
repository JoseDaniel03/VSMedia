# IPC interno — protocolo experimental v1

## Alcance

La capa Rust escucha **solo en 127.0.0.1** (puerto 3059 por defecto). Cada conexión transporta
una única petición con un entero de 32 bits big-endian para su longitud, seguido por JSON UTF-8.
Respuestas usan el mismo framing. Tamaño máximo: 4 MiB por mensaje.

La autenticación requiere un token diferente por plugin productor. El archivo JSON de tokens
se almacena localmente (en Linux con permisos `0600`) y **no se debe subir a Git**.
El token por sí mismo no autoriza namespaces: el catálogo SQLite también verifica permisos.
No se permiten mutaciones de permisos desde la API de red.

## Comandos

`ping`, `publish`, `lookup`, `read`, `publish_native`, `read_native`. Las solicitudes incluyen
`version`, `request_id`, `producer`, `token`, `command`.
Los comandos se etiquetan con `operation`. Cada respuesta incluye `version`,
`request_id` y exactamente uno de `result` / `error`.
Los recursos inline no deben superar 2 MiB. Los archivos de hasta 64 MiB
pueden usar sesiones temporales de transferencia por bloques de 256 KiB,
con verificación SHA-256, autorización por productor y límites de memoria.

Ejemplo (no utilizar tokens reales en documentación):

```json
{
  "version": 1,
  "request_id": "sample-1",
  "producer": "vsrender",
  "token": "TOKEN_LOCAL_DE_EJEMPLO_NO_REAL",
  "command": {
    "operation": "lookup",
    "key": {"namespace": "vsrender", "name": "weapons/sprite.png"}
  }
}
```

## Transferencias por bloques (experimental)

Para descargar: `begin_read(key)` devuelve `transfer_id`, `size` y `sha256`.
El cliente llama `read_chunk(transfer_id,offset)` en orden y finalmente
`end_read(transfer_id)` para liberar el recurso. El servidor valida el objeto
completo una sola vez al abrir la sesión. Java reconstruye y vuelve a verificar
el SHA-256 antes de devolver los bytes.

Para publicar archivos grandes: `begin_upload(key,kind,size,sha256)`
devuelve `transfer_id`. El productor manda fragmentos consecutivos mediante
`upload_chunk(transfer_id,offset,content_base64)`; cada respuesta devuelve
el siguiente `offset` aceptado. `complete_upload(transfer_id)` verifica
tamaño y SHA-256 antes de publicar en una transacción SQLite con permisos
por productor. `abort_upload(transfer_id)` descarta una sesión incompleta.

Límites v1:
- 64 MiB por archivo, 256 KiB por fragmento.
- 4 sesiones simultáneas de subida y 4 de bajada; 128 MiB reservados
  como máximo por tipo de sesión (cuotas separadas).
- Las sesiones inactivas caducan a los 120 segundos; se limpian
  al procesar nuevas operaciones sobre el registro.
- Las sesiones están vinculadas a la identidad del productor autenticado,
  no solo al identificador aleatorio del proceso.
- No hay reanudación tras reiniciar el servicio; los archivos se mantienen
  temporalmente en memoria, no en un archivo persistente de staging.
- `publish_native` y `read_native` siguen siendo manifiestos pequeños
  con límite inline de 2 MiB; el paquete `.vsbundle` todavía no tiene
  endpoint de importación/exportación por IPC.

**Nota de seguridad:** la validación de identidad ocurre antes de ejecutar
comandos, la descarga verifica derechos en cada fragmento y la publicación
comprueba los derechos dentro de la transacción final. Este protocolo solo
escucha en loopback y no debe exponerse públicamente.

## Formatos nativos

`publish_native` y `read_native` requieren `key`, `extension` (una de
`vsaudio`, `vsimage`, `vsstream`, `vspack`, `vsproject`) y, para escritura,
`content_base64`. Los manifiestos son JSON UTF-8 versionados con máximo 2 MiB;
Rust valida contenido, tipos, rutas y versión antes de publicarlo en el Core.
Los archivos finales siguen referenciando medios mediante hashes, no incluyen blobs.

El servicio utiliza las mismas ACL por productor/namespace y los mismos límites IPC.
`NativeFormatsApi` añade operaciones asíncronas a Java sin cambiar el contrato
`VSMediaApi` existente. Los vínculos entre recursos de otros namespaces todavía
no disponen de permisos delegados de lectura para validación automática; no se
comparte el contenido de esos recursos mediante el manifiesto.

## Java Bridge

`java/vsmedia-api` publica las interfaces que consumen otros plugins.
`java/vsmedia-bridge` implementa esas interfaces con Jackson y sockets loopback
asíncronos, sin bloquear regiones Folia. El adaptador de Bukkit y su ciclo de vida
todavía faltan, igual que el comando `buildResourcePack`, que actualmente falla
explícitamente como operación no implementada.

## Ejecución manual para desarrollo

```bash
chmod 600 ./local-tokens.json
cargo run -p vsmedia-engine -- grant ./data vsrender vsrender both
cargo run -p vsmedia-engine -- serve ./data ./local-tokens.json 3059
```

**Advertencias:** protocolo experimental, sin TLS (solo loopback), sin reanudación y sin transferencia fuera de memoria,
no apto para exposición pública. Las llamadas que acceden a SQLite no se ejecutan sobre
hilos Folia. No existe todavía un plugin de plataforma ni autenticación de VSClient.
