#!/usr/bin/env python3
"""Stage VSMedia Studio AutoMic v14 static files from the uploaded tar.gz."""
import argparse
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import tarfile
import tempfile

REQUIRED = (
    "index.html", "assets/js/core.js", "assets/js/simple.js",
    "assets/js/mix.js", "assets/js/radio.js", "assets/js/vspack.js",
    "modules/vspack/index.html",
    "modules/vspack/vs-model-studio/dist/bundle.js",
    "modules/vspack/pixel-studio/web/vs-pixel-studio.mjs",
)
STATIC = {"assets", "modules", "fonts", "images", "img", "icons", "static", "vendor"}
ROOT = {"index.html", "favicon.ico", "manifest.webmanifest", "robots.txt", "service-worker.js", "sw.js"}
EXCLUDE = {".git", ".github", "node_modules", "target", "tests", "rust", "__pycache__"}

def stage(archive: Path, destination: Path):
    archive, destination = archive.resolve(), destination.resolve()
    if "AutoMic-v14" not in archive.name or not tarfile.is_tarfile(archive):
        raise ValueError("Falta VSMedia-Studio-2026-AutoMic-v14 (1).tar.gz")
    if destination.exists() and (destination.is_symlink() or any(destination.iterdir())):
        raise ValueError("No se reemplaza un directorio web existente")
    with tarfile.open(archive, "r:gz") as source:
        files, total = [], 0
        for item in source:
            path = PurePosixPath(item.name)
            if path.is_absolute() or ".." in path.parts or "\\" in item.name:
                raise ValueError(f"Ruta insegura: {item.name}")
            if not item.isfile() and not item.isdir():
                raise ValueError(f"Enlace o entrada especial: {item.name}")
            if not item.isfile():
                continue
            total += item.size
            if item.size > 32*1024*1024 or total > 256*1024*1024 or len(files) >= 6000:
                raise ValueError("El paquete excede los límites de seguridad")
            files.append((path.parts, item))
        prefixes = [parts[:-1] for parts, item in files
                    if parts[-1:] == ("index.html",) and len(parts) <= 4
                    and any(p[:len(parts)+2] == parts[:-1]+("assets", "js", "core.js")
                            for p, _ in files)]
        if len(prefixes) != 1:
            raise ValueError("No se pudo identificar una única web completa")
        prefix = prefixes[0]
        selected = {}
        for parts, item in files:
            if parts[:len(prefix)] != prefix:
                continue
            rel = parts[len(prefix):]
            if not rel or any(x in EXCLUDE for x in rel):
                continue
            if rel[0] not in STATIC and "/".join(rel) not in ROOT:
                continue
            if rel[-1].endswith((".map", ".md", ".sh", ".py", ".rs", ".toml", ".lock")):
                continue
            key = "/".join(rel)
            if key.casefold() in selected:
                raise ValueError(f"Ruta duplicada: {key}")
            selected[key.casefold()] = (key, item)
        missing = [p for p in REQUIRED if p.casefold() not in selected]
        if missing:
            raise ValueError("Módulos faltantes: " + ", ".join(missing))
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.TemporaryDirectory(prefix=".vsmedia-web-", dir=destination.parent) as tmp:
            root = Path(tmp)
            for name, item in selected.values():
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                with source.extractfile(item) as inp, path.open("wb") as out:
                    shutil.copyfileobj(inp, out, 1024*1024)
            home = (root/"index.html").read_text("utf-8")
            if not all(x in home.lower() for x in ("home", "mix", "vspack")):
                raise ValueError("Index no corresponde a VSMedia Studio completa")
            # Remove only VSMedia's known legacy browser storage. Do not touch
            # unrelated Blockbench preferences or arbitrary localStorage keys.
            cleanup = root / "assets/js/vsmedia-legacy-cleanup.js"
            cleanup.parent.mkdir(parents=True, exist_ok=True)
            cleanup.write_text("""(() => {
  if (window.__VSMEDIA_LEGACY_CLEANED__) return;
  window.__VSMEDIA_LEGACY_CLEANED__ = true;
  try {
    const legacyDb = "vspack-local-v1";
    if (window.indexedDB) window.indexedDB.deleteDatabase(legacyDb);
  } catch (_) {}
  try {
    for (let i = localStorage.length - 1; i >= 0; i--) {
      const key = localStorage.key(i) || "";
      if (/^(vsmedia|vspack)([._:-]|$)/i.test(key)) localStorage.removeItem(key);
    }
  } catch (_) {}
})();
""", encoding="utf-8")
            bridge = Path(__file__).resolve().parent.parent / "studio/assets/minecraft.js"
            if not bridge.is_file():
                raise ValueError("Falta studio/assets/minecraft.js para integrar Minecraft")
            shutil.copyfile(bridge, root / "assets/js/vsmedia-minecraft.js")
            index = root / "index.html"
            html = index.read_text("utf-8")
            marker = '<script src="/assets/js/vsmedia-legacy-cleanup.js"></script>'
            if marker not in html:
                html = html.replace("</head>", marker + '<script src="/assets/js/vsmedia-minecraft.js"></script></head>', 1)
                if html == index.read_text("utf-8"):
                    html += "\n" + marker + '<script src="/assets/js/vsmedia-minecraft.js"></script>\n'
                index.write_text(html, encoding="utf-8")
            manifest = {"studio": "VSMedia Studio", "version": "AutoMic v14",
                        "source_archive": archive.name,
                        "source_sha256": hashlib.sha256(archive.read_bytes()).hexdigest(),
                        "static_files": len(selected),
                        "legacy_vspack_storage": "purged-on-first-load",
                        "minecraft_bridge": "enabled"}
            (root/"version.json").write_text(json.dumps(manifest, indent=2)+"\n", encoding="utf-8")
            if destination.exists():
                destination.rmdir()
            root.rename(destination)
    print(f"Web AutoMic v14 lista: {len(selected)} archivos; SHA256={manifest['source_sha256']}")

if __name__ == "__main__":
    p = argparse.ArgumentParser()
    p.add_argument("archive", type=Path)
    p.add_argument("destination", type=Path)
    args = p.parse_args()
    stage(args.archive, args.destination)
