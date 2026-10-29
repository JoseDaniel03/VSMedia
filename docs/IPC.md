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

`ping`, `publish`, `lookup`, `read`. Las solicitudes incluyen
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

## Ejecución manual para desarrollo

```bash
chmod 600 ./local-tokens.json
cargo run -p vsmedia-engine -- grant ./data vsrender vsrender both
cargo run -p vsmedia-engine -- serve ./data ./local-tokens.json 3059
```

**Advertencias:** protocolo experimental, sin TLS (solo loopback), sin transferencia masiva,
no apto para exposición pública. Las llamadas que acceden a SQLite no se ejecutan sobre
hilos Folia. No existe todavía un bridge de plataforma ni autenticación de VSClient.
