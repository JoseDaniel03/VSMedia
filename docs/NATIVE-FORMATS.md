# Formatos nativos VSMedia — esquema 1 (desarrollo)

Los formatos personalizados son **proyectos editables**, no códecs.
WebM encapsula video/audio, mientras que imágenes estáticas usan WebP/PNG/JPEG.
El motor sigue procesando/decodificando mediante bibliotecas existentes.

| Extensión | Contenido | Uso principal |
|---|---|---|
| `.vsaudio` | Timeline, fuentes pinneadas SHA-256, mezcla sin destruir originales | Audio Studio |
| `.vsimage` | Tamaño de lienzo, capas, opacidad, FPS y fuentes | Image Studio |
| `.vsstream` | Canal, playlist, buffer y espacialidad | Radio/streaming (todavía no operativo) |
| `.vspack` | Fuentes de assets, rutas de Resource Pack y prioridad | Pack Engine |
| `.vsproject` | Grafo de escenas/nodos, dependencias y referencias a otros formatos | Proyecto completo |

## Estructura de archivo

Todos son JSON UTF-8 limitados a 2 MiB por manifiesto, con la misma envoltura:

```json
{
  "magic": "VSMEDIA",
  "version": 1,
  "payload": {
    "kind": "project",
    "data": {
      "name": "Virgoland Cinematics",
      "nodes": [
        { "id": "intro", "document": { "key": {"namespace": "demo", "name": "intro"}, "kind": "audio", "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb" }, "depends_on": [] }
      ],
      "entry_points": ["intro"]
    }
  }
}
```

Los medios no van embebidos en el JSON: se referencian mediante `AssetKey` y SHA-256.
De esta forma múltiples escenas reutilizan un mismo recurso sin duplicar datos.
Los hashes aseguran que las referencias sean reproducibles; si un recurso cambia, el
proyecto puede actualizarse deliberadamente a su nueva revisión.

## Reglas de seguridad

- Validar extensión y `payload.kind` juntos (evita confundir un proyecto con una imagen).
- `magic` y `version` obligatorios: versiones futuras se rechazan hasta implementar migración.
- SHA-256 hex válido y claves saneadas; packs reutilizan validación de rutas del Pack Engine.
- `.vsproject`: nodos únicos, vínculos internos existentes, sin ciclos locales.
- Presupuesto de nodos/recursos, límites de FPS, opacidad, tamaño de documento y dimensiones.
- Referencias cruzadas a recursos existentes se pueden verificar contra el repositorio por hash.
- Una referencia a otro `.vsproject` es un enlace, no un empaquetado; los ciclos entre
  documentos **externos** necesitan un validador transitorio de grafo global (pendiente).
- No hay claves privadas de radio/voz ni credenciales incrustadas.

## Alcance actual

`vsmedia-formats` centraliza serialización, tipado y validación, usando el
`AudioProject` existente y la validación de destinos de `vsmedia-pack`.
Los documentos se publican/leen en `AssetRepository` como `AssetKind::Other` para
mantener compatible el contrato actual. Se añadirán tipos de recurso explícitos
cuando el contrato Java/Rust esté sincronizado.

**No existe aún soporte para exportar paquetes portables que incluyan todos los blobs**
ni integración de los formatos desde el panel web. Nunca asumir que un archivo
`.vsproject` por sí mismo contiene videos, imágenes o sonidos.
