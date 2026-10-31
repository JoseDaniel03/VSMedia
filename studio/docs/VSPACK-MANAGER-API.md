# VSPack Studio — contrato backend para el futuro gestor

El backend preparado se ejecuta sobre Rust/Axum, usando el motor
vsmedia-pack. No implementa una segunda web, otro mod ni un plugin.

## Flujo de la futura interfaz

1. Importar fuentes con POST /api/v1/packs/{id}.
2. Consultar GET /api/v1/packs/manager/capabilities para conocer límites.
3. Consultar GET /api/v1/packs/manager/recipe para obtener una receta inicial.
4. Guardar/editar esa receta JSON en la nueva interfaz.
5. Consultar POST /api/v1/packs/manager/plan para analizar una selección.
6. Descargar el pack combinado con POST /api/v1/packs/manager/export.
7. Opcionalmente solicitar split/plan y split/export para separar por categorías.

Los cuatro POST del manager aceptan el MISMO cuerpo JSON.

## Ejemplo JSON de receta

~~~json
{
  "schema": "vspack-recipe-v1",
  "name": "Virgoland 26.3 - UI",
  "sources": [
    {"fingerprint": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa", "enabled": true, "priority": 50},
    {"fingerprint": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb", "enabled": true, "priority": 20}
  ],
  "choices": {
    "assets/minecraft/textures/gui/sprites/tooltip/background.png": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
  },
  "exclude": []
}
~~~

Las huellas del ejemplo son ilustrativas. En la práctica, fingerprint
contiene el SHA-256 del archivo ZIP original que se importó. Si se
reimporta EXACTAMENTE el mismo ZIP, aunque cambie el ID de sesión,
su receta puede recuperarse. Reempaquetar el ZIP modifica esa huella.

choices asigna un archivo individual a una fuente concreta; así
podemos seleccionar los tooltips de MandalasGUI y conservar el resto
de la GUI de CozyUI, sin editar píxeles ni romper los archivos fuente.
Si la textura tiene un PNG.mcmeta acompañante, la receta permite
seleccionar también ese archivo.

## Rutas HTTP locales

| Método | Ruta | Propósito |
|---|---|---|
| GET | /api/v1/packs/manager/capabilities | Capacidades y categorías |
| GET | /api/v1/packs/manager/recipe | Receta portable de la sesión |
| POST | /api/v1/packs/manager/plan | Analiza la fusión de una receta |
| POST | /api/v1/packs/manager/export | Descarga Resource Pack ZIP unificado |
| POST | /api/v1/packs/manager/split/plan | Vista previa de separación por categoría |
| POST | /api/v1/packs/manager/split/export | Bundle ZIP con subpacks y hashes |

Para listar archivos/íconos, consultar candidatos de conflicto,
inspeccionar texturas y obtener recursos individuales, se conservan
las rutas anteriores documentadas en VSPACK-INTERFACES-API.md.

## División por categorías

- interface: GUI, HUD, tooltips, fuentes y atlas.
- items: texturas, modelos y definiciones de ítems/equipamiento.
- blocks: texturas/modelos/estados de bloques.
- audio: eventos y sonidos.
- particles: partículas.
- entities: recursos de entidades.
- languages: traducciones.
- other: metadatos y recursos auxiliares.

Cada ZIP interno generado tiene pack.mcmeta. Los archivos
PNG.mcmeta se mantienen junto a su textura por prefijo.
El bundle general también contiene manifest.json con SHA-1, SHA-256,
UUID y cantidad de archivos de cada subpack.

**Importante:** las referencias JSON entre categorías no se reescriben
automáticamente. Para que una interfaz funcione, puede necesitar sus
fuentes, atlas, modelos y texturas. Los subpacks se deben probar juntos
antes de publicarlos. El ZIP bundle no es un único pack instalable;
cada ZIP interno es un Resource Pack independiente.

## Límites y seguridad

- Hasta 24 packs por sesión y 96 MiB por ZIP comprimido.
- Hasta 12.000 entradas y 192 MiB descomprimidos por ZIP importado.
- 32 MiB por recurso y 384 MiB de fuentes descomprimidas en memoria.
- Hasta 65.536 contribuciones al merge estructurado.
- Recetas de máximo 64 KiB, hasta 512 elecciones y 8.192 exclusiones.
- ZIP con protección de path traversal, duplicados, enlaces simbólicos
  y tamaño descomprimido. Se admiten archivos README/License,
  RespackOpts JSON5 y miniaturas de raíz con extensiones limitadas.
- Todas las operaciones de fusión/exportación permanecen en Rust; no
  se transfieren archivos a una web de terceros.
- El bridge sigue limitado a localhost (127.0.0.1:3061).
  Los POST verifican que el Origin sea el Studio local permitido.

**Persistencia:** una receta JSON puede guardarse y volver a cargar,
pero el servidor NO conserva automáticamente los ZIP al reiniciar.
La nueva web podrá gestionar recetas sin tocar el motor; para un
catálogo persistente compartido hacen falta CAS, autenticación y
almacenamiento seguro.

**Minecraft:** este backend NO instala automáticamente paquetes en
VSClient ni publica a VSEngine/CanvasMC. Sigue pendiente una capa
de publicación HTTPS y autorización. Antes de redistribuir packs
de terceros deben revisarse sus licencias.

La interfaz web nueva queda para cuando llegue su código.
