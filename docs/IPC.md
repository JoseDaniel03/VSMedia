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
Los recursos inline no deben superar 2 MiB; los recursos más grandes necesitarán
un protocolo de transferencia por bloques todavía por desarrollar.

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

**Advertencias:** protocolo experimental, sin TLS (solo loopback), sin transferencia masiva,
no apto para exposición pública. Las llamadas que acceden a SQLite no se ejecutan sobre
hilos Folia. No existe todavía un plugin de plataforma ni autenticación de VSClient.
