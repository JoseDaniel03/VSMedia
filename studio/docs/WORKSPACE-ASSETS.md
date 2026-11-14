# VSAssets — biblioteca privada de archivos por proyecto

VSAssets es un gestor de archivos originales integrado en la web FullCanvas de
VSMedia Studio. **No agrega nada al Resource Pack de Minecraft automáticamente.**

## Uso desde la web

Abrí VSAssets en la navegación superior o en la tarjeta de Inicio.
Podés crear proyectos, crear carpetas, importar archivos o carpetas completas,
arrastrar archivos, buscar, renombrar/mover, eliminar, previsualizar cuando
el navegador lo soporte, descargar y exportar una copia ZIP completa.

Se admiten fuentes originales y archivos arbitrarios (.bbmodel, .blend, .gltf,
.obj, .psd, .webp, .png, .wav, .ogg, .mp4, .ttf, .json, .js, .py, etc.).
Los formatos desconocidos se guardan sin ejecutarse: vista previa opcional,
descarga siempre disponible. Si otro programa (VSRender, Blockbench,
Photoshop, Blender) modifica directamente los archivos de la carpeta física,
el botón **Sincronizar carpeta del disco** vuelve a registrar los nuevos
archivos y hashes SHA-256, siempre con confirmación explícita. No se convierte
automáticamente de Blockbench
a modelos Minecraft.

## Carpetas persistentes

Ruta configurable: VSMEDIA_DATA_DIR, o por defecto .vsmedia-data

    .vsmedia-data/
      workspace-projects/
        virgoland-1234/
          project.json
          assets/
            models/
            textures/
            audio/
            video/
            fonts/
            scripts/
            documents/
            other/
          exports/
      vspack-sources/      (separado)
      vspack-projects/     (separado)

El manifiesto project.json registra tamaño, ruta, fecha y SHA-256. Al leer,
verificar o exportar VSCore comprueba cada archivo. Las escrituras usan
archivos temporales y sustitución atómica. No se acepta un archivo corrupto
como si su revisión fuese válida.

## Seguridad y cuotas

- Máximo 32 proyectos, 2000 archivos y 512 MiB por proyecto; 96 MiB por archivo.
- Se rechazan rutas absolutas, .., caracteres peligrosos, nombres reservados
  de Windows, enlaces simbólicos y archivos especiales.
- Reemplazar un archivo requiere autorización expresa en la interfaz.
- Todo permanece local a VSCore. No es un Drive público multiusuario.
- No hay distribución automática a VSPack, CDN ni Minecraft.

## Endpoints HTTP locales

- GET /api/v1/workspace/projects: catálogo de proyectos
- POST /api/v1/workspace/projects/{id}: crear proyecto, JSON con name
- GET /api/v1/workspace/projects/{id}: carpetas y recursos
- POST /api/v1/workspace/projects/{id}/folders: JSON con path
- POST /api/v1/workspace/projects/{id}/files?path=assets/models/arma.bbmodel:
  bytes del archivo; añadir &replace=true solo para sustitución autorizada
- GET /api/v1/workspace/projects/{id}/file?path=...: descarga verificada
- DELETE /api/v1/workspace/projects/{id}/file?path=...: eliminar
- POST /api/v1/workspace/projects/{id}/move: JSON con from y to
- GET /api/v1/workspace/projects/{id}/verify: comprobar integridad SHA-256
- POST /api/v1/workspace/projects/{id}/rescan: registrar cambios externos en disco
- GET /api/v1/workspace/projects/{id}/export: ZIP en streaming

El ZIP de respaldo contiene project.json y originales, **no un Resource Pack**.

## Alcance

La biblioteca es persistente y tiene respaldo exportable. Todavía no incluye
sincronización cloud, gestión multiusuario ni importación automática de ZIPs
de respaldo con preservación de los IDs antiguos (aunque se puede descomprimir
y reimportar manualmente). La publicación en Minecraft se maneja por VSPack
con una acción explícita, fuera de esta biblioteca.

No se construyen ni distribuyen JARs en esta tanda.
