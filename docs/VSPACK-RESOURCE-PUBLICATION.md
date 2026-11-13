# VSPack — publicación de texturas, modelos y recursos (CanvasMC/Folia 26.3)

## Publicar desde VSMedia

En la web **original** abrir **✦ Minecraft · Publicar → Recursos / VSPack**.
Esta pestaña adicional, sin sustituir la interfaz FullCanvas, admite ZIPs
Minecraft y carpetas con la estructura habitual de Resource Packs.

Se admiten texturas **PNG**, modelos JSON de bloques e ítems, definiciones
de ítems y blockstates, atlas, animaciones PNG.mcmeta, sprites de GUI/HUD,
equipamiento, entidades, fuentes, partículas, shaders, traducciones y
sonidos OGG con eventos sounds.json.

Los archivos editables de Blockbench **.bbmodel** no son modelos JSON
de Minecraft. Se deben exportar al formato compatible con el cliente
desde Blockbench. Tampoco se convierten automáticamente archivos Blender,
FBX, OBJ ni GLTF.

### Estructura de ejemplo

~~~text
Mi-Pack/
├── pack.mcmeta
├── pack.png
└── assets/
    └── virgoland/
        ├── textures/item/arma.png
        ├── textures/item/arma.png.mcmeta
        ├── models/item/arma.json
        ├── items/arma.json
        ├── textures/gui/icono.png
        ├── blockstates/...
        ├── textures/entity/...
        ├── font/hud.json
        ├── atlases/...
        ├── particles/...
        ├── shaders/...
        └── sounds.json
~~~

VSMedia conserva los formatos declarados en pack.mcmeta. Se debe usar
el pack format compatible con la versión real de Minecraft; el publicador
no inventa números de versión.

### Pasos

1. Elegir **Importar ZIP** o **Importar carpeta**. La carpeta se empaqueta
   en el navegador como ZIP sin recomprimir PNG/OGG, fuera de Folia.
   La carpeta requiere pack.mcmeta y assets/. Hasta 96 MiB por ZIP.
2. VSCore valida rutas y cuotas, importa y guarda el ZIP original en
   VSMEDIA_DATA_DIR/vspack-sources/ con SHA-256.
3. Configurar prioridad con **Orden**, quitar fuentes innecesarias y
   consultar **Revisar dependencias** para conflictos y referencias.
   Las fuentes, prioridades y elecciones sobreviven a reinicios.
4. **Exportar pack** descarga un Resource Pack combinado. También
   incluye sonidos publicados desde VSAudio/VSMix y fuentes TTF/OTF
   añadidas a la biblioteca de Minecraft de VSMedia.

## Integridad de la publicación

El publicador offline tools/vspack_publish.py optimiza y divide el ZIP
por categoría y tamaño, pero **verifica que ninguna textura, modelo,
animación, archivo JSON, font ni evento de sonido desaparezca**.

Cada release tiene manifest.json con inventario por tipo de recurso
y resource_graph_sha256. El verificador reconstruye lógicamente todas
las capas y compara el resultado con el pack optimizado. Los eventos
sounds.json se reúnen por ID, y los archivos duplicados entre capas
deben tener exactamente los mismos bytes.

El nuevo informe preflight advierte (sin borrar contenidos) sobre
modelos de Blockbench no convertidos, PNG con firma inválida,
JSON mal formados y referencias a recursos ausentes. Una referencia
faltante puede venir de Minecraft vanilla u otro mod; se informa
para revisarla en lugar de suponer que el pack está roto.

## Entregar el pack a Minecraft

La exportación del ZIP no equivale a una distribución instalada.
Para la entrega automática se usa el monitor externo, separado de
los schedulers del servidor:

~~~bash
python3 tools/vspack_publish.py watch ./publicaciones \
  --interval-seconds 300 --max-mib 90 --profile balanced --publish-r2
~~~

El monitor publica los ZIPs como URLs HTTPS inmutables y cambia
current.json solamente después de validar todos los archivos.
El plugin Folia comprueba ese manifest remoto cuando la opción
publication.enabled está activada y ofrece las capas a los jugadores.
Los clientes las descargan desde el CDN, no desde la memoria de Folia.

La configuración de R2 y los secretos se explican en
VSPACK-AUTO-PUBLISH.md. Se admiten hasta 31 capas publicadas remotamente
y 32 en total, contando bases y packs aportados por otros plugins.

**VSRadio por Plasmo Voice** es independiente del pack de recursos.
Los sonidos pregrabados, modelos y texturas sí van al Resource Pack.

## Límites pendientes

Todavía no se verificó una subida real a Cloudflare R2 con credenciales
del proyecto, ni se probaron los modelos y shaders específicos en un
cliente real de CanvasMC/Folia 26.3. No se realiza conversión automática
de BBModel a JSON Minecraft. El resultado de las pruebas unitarias no
sustituye validar descarga y visualización en el juego.

Esta tanda modifica código y pruebas; **no genera JARs y no fusiona
la PR de desarrollo a main**.
