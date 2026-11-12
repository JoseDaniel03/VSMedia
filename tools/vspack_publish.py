#!/usr/bin/env python3
"""VSPack offline optimizer, size-aware publisher and R2 uploader.

No JAR compilation, Minecraft server threads or credentials in repository.
PackSquash (if installed) is invoked OUTSIDE Minecraft. Python stdlib for build
and verify; boto3 is only needed when actually publishing to Cloudflare R2.
"""
from __future__ import annotations

import argparse
import hashlib
import io
import json
import os
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
import tempfile
import uuid
import zipfile

MAX_UNPACKED = 512 * 1024 * 1024
MAX_FILES = 12000
MAX_FILE = 128 * 1024 * 1024
PACK_UUID_NAMESPACE = uuid.UUID("cd4566b8-5dd6-55bf-a95f-a4b8a32e275c")
ORDER = ("base", "blocks", "items", "entities", "particles", "interface", "audio", "languages")


class PackError(ValueError):
    pass


def digest(data: bytes, algorithm: str) -> str:
    return hashlib.new(algorithm, data).hexdigest()


def valid_path(name: str) -> bool:
    path = PurePosixPath(name)
    return (bool(name) and not name.startswith("/") and "\\" not in name
            and ":" not in name and "\x00" not in name
            and all(part not in ("", ".", "..") for part in name.split("/"))
            and str(path) == name)


def inspect_zip(data: bytes) -> dict[str, bytes]:
    if len(data) > MAX_UNPACKED:
        raise PackError("ZIP original mayor a 512 MiB")
    result: dict[str, bytes] = {}
    try:
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            members = [m for m in archive.infolist() if not m.is_dir()]
            if len(members) > MAX_FILES:
                raise PackError("Demasiados archivos en el Resource Pack")
            total = 0
            for info in members:
                if not valid_path(info.filename) or info.filename in result:
                    raise PackError("Ruta duplicada o insegura: " + info.filename)
                mode = info.external_attr >> 16
                if stat.S_IFMT(mode) == stat.S_IFLNK:
                    raise PackError("No se permiten enlaces simbólicos")
                if info.file_size > MAX_FILE or (info.compress_size and
                    info.file_size > info.compress_size * 300):
                    raise PackError("Archivo o ratio ZIP potencialmente peligroso: " + info.filename)
                total += info.file_size
                if total > MAX_UNPACKED:
                    raise PackError("ZIP expandido supera los 512 MiB")
                result[info.filename] = archive.read(info)
    except (zipfile.BadZipFile, EOFError) as exc:
        raise PackError("ZIP inválido: " + str(exc)) from exc
    if "pack.mcmeta" not in result:
        raise PackError("pack.mcmeta debe estar en la raíz del ZIP")
    try:
        metadata = json.loads(result["pack.mcmeta"])
    except (ValueError, UnicodeDecodeError) as exc:
        raise PackError("pack.mcmeta JSON inválido") from exc
    if not isinstance(metadata, dict) or not isinstance(metadata.get("pack"), dict):
        raise PackError("pack.mcmeta necesita un objeto pack válido")
    return result


def normalized(entries: dict[str, bytes]) -> dict[str, bytes]:
    out = dict(entries)
    for path, data in entries.items():
        if path.endswith((".json", ".mcmeta")):
            try:
                doc = json.loads(data)
                out[path] = json.dumps(doc, ensure_ascii=False,
                    separators=(",", ":"), sort_keys=True).encode("utf-8")
            except (ValueError, UnicodeDecodeError):
                pass  # A third-party pack may use JSONC or a custom format.
    return out


def optimize(entries: dict[str, bytes], profile: str, executable: str | None) -> tuple[dict[str, bytes], list[str]]:
    notes: list[str] = []
    entries = normalized(entries)
    if profile == "lossless":
        notes.append("JSON minificado y ZIP DEFLATE; audio y PNG conservados sin pérdidas")
        return entries, notes
    binary = executable or shutil.which("packsquash")
    if not binary:
        notes.append("PackSquash no instalado: compresión ZIP/JSON; OGG/PNG sin transcodificar")
        return entries, notes
    with tempfile.TemporaryDirectory(prefix="vspack-opt-") as tmp:
        root = Path(tmp) / "input"
        root.mkdir()
        output = Path(tmp) / "optimized.zip"
        for path, data in entries.items():
            dest = root.joinpath(*path.split("/"))
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(data)
        conf = Path(tmp) / "packsquash.toml"
        # JSON strings are valid TOML basic strings for normalized paths.
        lines = [
            "pack_directory = " + json.dumps(str(root)),
            "output_file_path = " + json.dumps(str(output)),
        ]
        if profile == "compact":
            lines += ["recompress_compressed_files = true", "zip_compression_iterations = 8"]
        conf.write_text("\n".join(lines) + "\n", encoding="utf-8")
        try:
            subprocess.run([binary, str(conf)], check=True, timeout=600,
                           stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
            candidate = inspect_zip(output.read_bytes())
            dropped = sorted(set(entries) - set(candidate))
            if dropped:
                notes.append("PackSquash omitió recursos, se conservó el ZIP original para no romperlos: "
                             + ", ".join(dropped[:4]))
                return entries, notes
            notes.append("PackSquash aplicado: " + profile)
            return candidate, notes
        except (OSError, subprocess.SubprocessError, PackError) as exc:
            notes.append("PackSquash no completó: " + str(exc)[:250] + ". Se conservan recursos originales.")
            return entries, notes


def category(path: str) -> str:
    segments = path.split("/")
    tail = "/".join(segments[2:]) if len(segments) >= 3 and segments[0] == "assets" else ""
    if tail.startswith(("textures/gui/", "textures/ui/", "textures/hud/",
                        "textures/font/", "font/", "atlases/", "textures/mob_effect/")):
        return "interface"
    if tail.startswith(("textures/item/", "models/item/", "items/", "equipment/")):
        return "items"
    if tail.startswith(("textures/block/", "models/block/", "blockstates/")):
        return "blocks"
    if tail.startswith(("sounds/",)) or tail == "sounds.json":
        return "audio"
    if tail.startswith(("particles/", "textures/particle/")):
        return "particles"
    if tail.startswith(("textures/entity/", "models/entity/", "entity/")):
        return "entities"
    if tail.startswith("lang/"):
        return "languages"
    return "base"


class Unit:
    def __init__(self, group: str, files: dict[str, bytes] | None = None,
                 events: dict[str, dict] | None = None) -> None:
        self.group = group
        self.files = files or {}
        self.events = events or {}

    @property
    def weight(self) -> int:
        return sum(map(len, self.files.values())) + sum(len(json.dumps(e)) for e in self.events.values())


def sound_ref(ref: object, namespace: str) -> str | None:
    name = ref if isinstance(ref, str) else ref.get("name") if isinstance(ref, dict) else None
    if not isinstance(name, str):
        return None
    if ":" in name:
        namespace, name = name.split(":", 1)
    path = "assets/" + namespace + "/sounds/" + name + ".ogg"
    return path if valid_path(path) else None


def units_for(entries: dict[str, bytes]) -> dict[str, list[Unit]]:
    units: dict[str, list[Unit]] = {}
    used: set[str] = {"pack.mcmeta", "pack.png"}
    for path, data in sorted(entries.items()):
        if not (path.startswith("assets/") and path.endswith("/sounds.json")):
            continue
        namespace = path.split("/")[1]
        try:
            events = json.loads(data)
            if not isinstance(events, dict):
                raise ValueError("sounds.json no es un objeto")
        except (ValueError, UnicodeDecodeError) as exc:
            raise PackError("sounds.json inválido: " + path) from exc
        used.add(path)
        for event_id, event in sorted(events.items()):
            if not isinstance(event, dict):
                raise PackError("Definición de evento inválida: " + path + "/" + event_id)
            refs = event.get("sounds", [])
            if not isinstance(refs, list):
                raise PackError("Evento sin lista sounds válida: " + str(event_id))
            files = {}
            for item in refs:
                ref = sound_ref(item, namespace)
                if ref in entries:
                    files[ref] = entries[ref]
                    used.add(ref)
            units.setdefault("audio", []).append(Unit("audio", files, {namespace: {event_id: event}}))
        if not events:
            units.setdefault("audio", []).append(Unit("audio", {path: data}))
    for path, data in sorted(entries.items()):
        if path in used:
            continue
        group = category(path)
        companions = {path: data}
        if path.endswith(".mcmeta") and path[:-7] in entries:
            continue
        sidecar = path + ".mcmeta"
        if sidecar in entries and sidecar not in used:
            companions[sidecar] = entries[sidecar]
            used.add(sidecar)
        used.add(path)
        units.setdefault(group, []).append(Unit(group, companions))
    return units


def archive_for(units: list[Unit], metadata: dict[str, bytes]) -> bytes:
    files = dict(metadata)
    events: dict[str, dict] = {}
    for unit in units:
        for name, data in unit.files.items():
            if name in files and files[name] != data:
                raise PackError("Colisión de contenido: " + name)
            files[name] = data
        for namespace, definitions in unit.events.items():
            for event, desc in definitions.items():
                old = events.setdefault(namespace, {}).setdefault(event, desc)
                if old != desc:
                    raise PackError("Eventos de sonido incompatibles: " + event)
    for namespace, definitions in events.items():
        dest = "assets/" + namespace + "/sounds.json"
        existing = json.loads(files[dest]) if dest in files else {}
        existing.update(definitions)
        files[dest] = json.dumps(existing, ensure_ascii=False, sort_keys=True,
                                 separators=(",", ":")).encode("utf-8")
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", compression=zipfile.ZIP_DEFLATED,
                         compresslevel=9, allowZip64=False) as archive:
        for name, data in sorted(files.items()):
            info = zipfile.ZipInfo(name, date_time=(1980, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            archive.writestr(info, data, compress_type=zipfile.ZIP_DEFLATED,
                             compresslevel=9)
    return output.getvalue()


def fitted(units: list[Unit], metadata: dict[str, bytes], cap: int) -> list[bytes]:
    zipped = archive_for(units, metadata)
    if len(zipped) <= cap:
        return [zipped]
    if len(units) < 2:
        raise PackError("Un recurso/evento indivisible excede el límite por pack: "
                        + str(round(len(zipped) / (1024 * 1024), 2)) + " MiB. "
                        "Recomprimí o dividí el audio antes de publicar.")
    # Balance roughly by uncompressed cost; validate actual ZIP size recursively.
    a: list[Unit] = []
    b: list[Unit] = []
    size_a = size_b = 0
    for unit in sorted(units, key=lambda entry: entry.weight, reverse=True):
        if size_a <= size_b:
            a.append(unit)
            size_a += unit.weight
        else:
            b.append(unit)
            size_b += unit.weight
    return fitted(a, metadata, cap) + fitted(b, metadata, cap)


def build(input_zip: Path, destination: Path, limit_mib: int,
          profile: str, packsquash: str | None, require_optimizer: bool) -> dict:
    cap = limit_mib * 1024 * 1024
    if not 1 <= limit_mib <= 250:
        raise PackError("Límite permitido: entre 1 y 250 MiB")
    source = inspect_zip(input_zip.read_bytes())
    optimized, notes = optimize(source, profile, packsquash)
    if require_optimizer and profile != "lossless" and not any(
            entry.startswith("PackSquash aplicado") for entry in notes):
        raise PackError("Se pidió PackSquash obligatorio, pero no pudo ejecutarse: " + "; ".join(notes))
    metadata = {"pack.mcmeta": optimized["pack.mcmeta"]}
    if "pack.png" in optimized:
        metadata["pack.png"] = optimized["pack.png"]
    directory = destination
    if directory.exists() and any(directory.iterdir()):
        raise PackError("El directorio de salida debe estar vacío (evita pisar versiones anteriores)")
    directory.mkdir(parents=True, exist_ok=True)
    packs = []
    grouped = units_for(optimized)
    for group in ORDER:
        entries = grouped.get(group, [])
        if not entries:
            continue
        archives = fitted(entries, metadata, cap)
        for index, data in enumerate(archives, start=1):
            name = group + "-" + str(index).zfill(2)
            sha256 = digest(data, "sha256")
            filename = name + "-" + sha256[:12] + ".zip"
            (directory / filename).write_bytes(data)
            packs.append({
                "logical_id": name, "uuid": str(uuid.uuid5(PACK_UUID_NAMESPACE, name)),
                "filename": filename, "bytes": len(data),
                "sha1": digest(data, "sha1"), "sha256": sha256,
                "category": group, "priority": len(packs),
                "url": None,
            })
    if not packs:
        packs_data = archive_for([], metadata)
        (directory / "base-01.zip").write_bytes(packs_data)
        packs.append({"logical_id": "base-01", "uuid": str(uuid.uuid5(PACK_UUID_NAMESPACE, "base-01")),
                      "filename": "base-01.zip", "bytes": len(packs_data),
                      "sha1": digest(packs_data, "sha1"), "sha256": digest(packs_data, "sha256"),
                      "category": "base", "priority": 0, "url": None})
    if len(packs) >= 32:
        raise PackError("Más de 31 packs en una publicación; aumentá el límite por módulo "
                        "o reducí contenido para que Folia pueda ofrecerlos juntos")
    report = {
        "schema": "vspack-publication-v1", "status": "built_not_published",
        "profile": profile, "max_pack_mib": limit_mib,
        "original_uncompressed_bytes": sum(map(len, source.values())),
        "optimized_uncompressed_bytes": sum(map(len, optimized.values())),
        "notes": notes,
        "warning": "Revisar dependencias entre categorías antes de cambiar el orden. "
                   "Las capas se apilan de base a audio y UI, las actualizaciones requieren hashes nuevos.",
        "packs": packs,
    }
    (directory / "manifest.json").write_text(
        json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    verify(directory)
    return report


def verify(directory: Path) -> dict:
    manifest_path = directory / "manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    if manifest.get("schema") != "vspack-publication-v1":
        raise PackError("Manifest no compatible")
    if not isinstance(manifest.get("packs"), list) or not manifest["packs"]:
        raise PackError("Manifest sin packs")
    if len(manifest["packs"]) >= 32:
        raise PackError("Minecraft/VSMedia admite hasta 31 capas remotas por publicación")
    seen: set[str] = set()
    for pack in manifest["packs"]:
        name = pack["filename"]
        if not valid_path(name) or "/" in name or name in seen:
            raise PackError("Nombre de pack duplicado o inseguro")
        seen.add(name)
        payload = (directory / name).read_bytes()
        if (len(payload) != pack["bytes"] or digest(payload, "sha1") != pack["sha1"]
                or digest(payload, "sha256") != pack["sha256"]):
            raise PackError("Hash o tamaño inválido en " + name)
        if len(payload) > manifest["max_pack_mib"] * 1024 * 1024:
            raise PackError("Pack excede límite: " + name)
        inspect_zip(payload)
    return manifest


def r2_settings(prefix: str):
    base = os.environ.get("VSPACK_R2_PUBLIC_URL", "").rstrip("/")
    endpoint = os.environ.get("VSPACK_R2_ENDPOINT", "")
    bucket = os.environ.get("VSPACK_R2_BUCKET", "")
    access = os.environ.get("VSPACK_R2_ACCESS_KEY_ID", "")
    secret = os.environ.get("VSPACK_R2_SECRET_ACCESS_KEY", "")
    if not base.startswith("https://") or not all((endpoint.startswith("https://"),
                                                    bucket, access, secret)):
        raise PackError("Falta configuración privada R2: VSPACK_R2_ENDPOINT, BUCKET, "
                        "PUBLIC_URL (https), ACCESS_KEY_ID, SECRET_ACCESS_KEY")
    if not prefix or not all(ch.isalnum() or ch in "-_/" for ch in prefix) or ".." in prefix:
        raise PackError("Prefijo R2 inseguro")
    try:
        import boto3
    except ImportError as exc:
        raise PackError("Para subir a R2 instalá boto3: python -m pip install boto3") from exc
    client = boto3.client("s3", endpoint_url=endpoint,
        aws_access_key_id=access, aws_secret_access_key=secret, region_name="auto")
    return client, bucket, base


def publish_r2(directory: Path, prefix: str) -> dict:
    manifest = verify(directory)
    client, bucket, base = r2_settings(prefix)
    published = json.loads(json.dumps(manifest))
    for pack in published["packs"]:
        key = prefix.rstrip("/") + "/versions/" + pack["filename"]
        data = (directory / pack["filename"]).read_bytes()
        client.put_object(Bucket=bucket, Key=key, Body=data,
            ContentType="application/zip",
            CacheControl="public, max-age=31536000, immutable",
            Metadata={"sha256": pack["sha256"], "sha1": pack["sha1"]})
        pack["url"] = base + "/" + key
    published["status"] = "uploaded_to_r2"
    # Upload a pointer LAST so incomplete versions never become current.
    published_key = prefix.rstrip("/") + "/current.json"
    payload = (json.dumps(published, ensure_ascii=False, separators=(",", ":")) + "\n").encode("utf-8")
    client.put_object(Bucket=bucket, Key=published_key, Body=payload,
        ContentType="application/json", CacheControl="no-cache, max-age=0")
    (directory / "published-manifest.json").write_bytes(payload)
    print("Manifest publicado: " + base + "/" + published_key)
    return published




def rollback_r2(directory: Path, prefix: str) -> dict:
    """Restore a previously published, locally verified manifest, without re-uploading ZIPs."""
    built = verify(directory)
    previous_file = directory / "published-manifest.json"
    published = json.loads(previous_file.read_text(encoding="utf-8"))
    if published.get("schema") != "vspack-publication-v1" or published.get("status") != "uploaded_to_r2":
        raise PackError("No hay una publicación anterior válida en esa carpeta")
    if [(p["filename"], p["sha256"]) for p in built["packs"]] != [
        (p["filename"], p["sha256"]) for p in published["packs"]]:
        raise PackError("El manifiesto publicado no coincide con los ZIPs locales")
    client, bucket, base = r2_settings(prefix)
    for pack in published["packs"]:
        if not isinstance(pack.get("url"), str) or not pack["url"].startswith(
                base + "/" + prefix.rstrip("/") + "/versions/"):
            raise PackError("Un ZIP de la versión anterior no pertenece al CDN configurado")
    key = prefix.rstrip("/") + "/current.json"
    client.put_object(Bucket=bucket, Key=key, Body=previous_file.read_bytes(),
        ContentType="application/json", CacheControl="no-cache, max-age=0")
    print("Versión anterior restaurada: " + base + "/" + key)
    return published


def download_studio_export(url: str) -> bytes:
    """Allow only loopback endpoints; never fetch a user-controlled Internet URL."""
    from urllib.parse import urlsplit
    from urllib.request import build_opener, HTTPRedirectHandler, ProxyHandler
    parsed = urlsplit(url)
    if (parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost")
            or parsed.port not in (3060, 3061) or parsed.username is not None
            or parsed.password is not None or parsed.query or parsed.fragment
            or parsed.path != "/api/v1/packs/export"):
        raise PackError("Solo se permite importar desde VSMedia local :3060 o :3061")
    class NoRedirect(HTTPRedirectHandler):
        def redirect_request(self, request, fp, code, msg, headers, newurl):
            return None
    opener = build_opener(ProxyHandler({}), NoRedirect())
    with opener.open(url, timeout=60) as response:
        if response.status != 200:
            raise PackError("VSCore no respondió 200 al exportar")
        data = response.read(MAX_UNPACKED + 1)
    if len(data) > MAX_UNPACKED:
        raise PackError("Resource Pack exportado supera la cuota de 512 MiB")
    inspect_zip(data)
    return data


def studio_session_pack_count(studio_url: str) -> int | None:
    """Best-effort guard against accidentally publishing missing ephemeral VSPack sources."""
    from urllib.parse import urlsplit
    from urllib.request import build_opener, HTTPRedirectHandler, ProxyHandler
    from urllib.error import URLError
    parsed = urlsplit(studio_url)
    if (parsed.scheme != "http" or parsed.hostname not in ("127.0.0.1", "localhost")
            or parsed.port not in (3060, 3061)
            or parsed.path != "/api/v1/packs/export" or parsed.query or parsed.fragment):
        raise PackError("URL de VSCore fuera de loopback")
    class NoRedirect(HTTPRedirectHandler):
        def redirect_request(self, request, fp, code, msg, headers, newurl):
            return None
    opener = build_opener(ProxyHandler({}), NoRedirect())
    try:
        with opener.open(studio_url.removesuffix("/export"), timeout=5) as response:
            data = response.read(65537)
        if len(data) > 65536:
            return None
        packs = json.loads(data).get("packs")
        return len(packs) if isinstance(packs, list) else None
    except (OSError, URLError, ValueError, KeyError, TypeError):
        return None


def watch_once(root: Path, studio_url: str, limit: int, profile: str,
               packsquash: str | None, require_optimizer: bool, publish: bool,
               prefix: str) -> dict:
    """Observe VSMedia's deterministic ZIP. Preserve the last good publication on failure."""
    import time
    source_bytes = download_studio_export(studio_url)
    sha256 = digest(source_bytes, "sha256")
    root.mkdir(parents=True, exist_ok=True)
    state_file = root / "publication-state.json"
    state = json.loads(state_file.read_text("utf-8")) if state_file.exists() else {}
    count = studio_session_pack_count(studio_url)
    if (state.get("source_pack_count", 0) > 0 and count == 0):
        raise PackError("Los packs importados ya no están cargados en la sesión VSPack. "
                        "Reimportá las fuentes originales antes de publicar esta revisión.")
    if state.get("source_sha256") == sha256 and (
            not publish or state.get("status") == "uploaded_to_r2"):
        return {"changed": False, "source_sha256": sha256, "status": state.get("status")}
    destination = root / ("revision-" + sha256[:16])
    if destination.exists():
        if not (destination / "manifest.json").is_file():
            raise PackError("Ya existe una carpeta de revisión sin manifest válido: " + str(destination))
        existing = verify(destination)
        if existing.get("profile") != profile or existing.get("max_pack_mib") != limit:
            raise PackError("Esta revisión ya existe con otra configuración; usá otra carpeta de publicaciones")
    else:
        with tempfile.TemporaryDirectory(prefix=".vspack-staging-", dir=root) as tmp:
            src = Path(tmp) / "studio.zip"
            src.write_bytes(source_bytes)
            staged = Path(tmp) / "output"
            build(src, staged, limit, profile, packsquash, require_optimizer)
            # Promote an entire successfully validated revision, never partial ZIPs.
            os.replace(staged, destination)
    if publish:
        published = publish_r2(destination, prefix)
        status = published["status"]
    else:
        status = "built_not_published"
    next_state = {"schema": "vspack-watch-v1", "source_sha256": sha256,
                  "source_pack_count": count if count is not None else state.get("source_pack_count", 0),
                  "revision": destination.name, "status": status}
    temporary = state_file.with_suffix(".tmp")
    temporary.write_text(json.dumps(next_state, indent=2) + "\n", "utf-8")
    os.replace(temporary, state_file)
    return {"changed": True, **next_state}


def watch_loop(args) -> None:
    """Long-running opt-in daemon. Not run on Minecraft/Folia threads."""
    import time
    interval = max(60, min(args.interval_seconds, 3600))
    print("VSPack watch: intervalo", interval, "segundos; Ctrl+C para detener", flush=True)
    while True:
        try:
            report = watch_once(args.directory, args.studio_url, args.max_mib,
                                args.profile, args.packsquash, args.require_packsquash,
                                args.publish_r2, args.prefix)
            print("VSPack watch:", "nueva versión" if report["changed"] else "sin cambios",
                  report["source_sha256"][:12], report["status"], flush=True)
        except (PackError, OSError, KeyError, ValueError) as exc:
            print("VSPack watch: se conserva la última publicación válida: " + str(exc),
                  file=sys.stderr, flush=True)
            if args.once:
                raise
        if args.once:
            break
        time.sleep(interval)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="VSPack: optimizar, dividir y publicar packs sin generar JAR")
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build", help="Construir ZIPs independientes + SHA-1/UUID")
    build_parser.add_argument("source", type=Path, help="ZIP exportado desde VSMedia")
    build_parser.add_argument("destination", type=Path, help="Carpeta nueva de salida")
    build_parser.add_argument("--max-mib", type=int, default=90, help="90 por defecto; 95 para hosting reducido")
    build_parser.add_argument("--profile", choices=("lossless", "balanced", "compact"), default="balanced")
    build_parser.add_argument("--packsquash", help="Ruta ejecutable PackSquash (opcional)")
    build_parser.add_argument("--require-packsquash", action="store_true")
    studio_parser = commands.add_parser("studio", help="Leer el ZIP directamente de VSCore local, construir y opcionalmente publicar")
    studio_parser.add_argument("destination", type=Path)
    studio_parser.add_argument("--studio-url", default="http://127.0.0.1:3061/api/v1/packs/export")
    studio_parser.add_argument("--max-mib", type=int, default=90)
    studio_parser.add_argument("--profile", choices=("lossless", "balanced", "compact"), default="balanced")
    studio_parser.add_argument("--packsquash")
    studio_parser.add_argument("--require-packsquash", action="store_true")
    studio_parser.add_argument("--publish-r2", action="store_true", help="Requiere secretos R2 en variables de entorno")
    studio_parser.add_argument("--prefix", default="virgoland")
    watch_parser = commands.add_parser("watch", help="Observar VSCore y construir/publicar automáticamente cada revisión nueva")
    watch_parser.add_argument("directory", type=Path, help="Directorio persistente de versiones")
    watch_parser.add_argument("--studio-url", default="http://127.0.0.1:3061/api/v1/packs/export")
    watch_parser.add_argument("--interval-seconds", type=int, default=300)
    watch_parser.add_argument("--max-mib", type=int, default=90)
    watch_parser.add_argument("--profile", choices=("lossless", "balanced", "compact"), default="balanced")
    watch_parser.add_argument("--packsquash")
    watch_parser.add_argument("--require-packsquash", action="store_true")
    watch_parser.add_argument("--publish-r2", action="store_true")
    watch_parser.add_argument("--prefix", default="virgoland")
    watch_parser.add_argument("--once", action="store_true", help="Una iteración para pruebas y cron")
    verify_parser = commands.add_parser("verify", help="Validar hashes, rutas y límites de una publicación")
    verify_parser.add_argument("directory", type=Path)
    rollback_parser = commands.add_parser("rollback-r2", help="Restaurar el manifest de una versión publicada anteriormente")
    rollback_parser.add_argument("directory", type=Path, help="Carpeta anterior con published-manifest.json")
    rollback_parser.add_argument("--prefix", default="virgoland")
    publish_parser = commands.add_parser("publish-r2", help="Subir versión inmutable a Cloudflare R2")
    publish_parser.add_argument("directory", type=Path)
    publish_parser.add_argument("--prefix", default="virgoland")
    args = parser.parse_args(argv)
    try:
        if args.command == "watch":
            watch_loop(args)
        elif args.command == "studio":
            with tempfile.TemporaryDirectory(prefix="vspack-studio-") as folder:
                source = Path(folder) / "vsmedia-export.zip"
                source.write_bytes(download_studio_export(args.studio_url))
                report = build(source, args.destination, args.max_mib, args.profile,
                               args.packsquash, args.require_packsquash)
            print("Pack de Studio listo:", len(report["packs"]), "capas")
            for note in report["notes"]:
                print(" Nota:", note)
            if args.publish_r2:
                publish_r2(args.destination, args.prefix)
        elif args.command == "build":
            report = build(args.source, args.destination, args.max_mib,
                           args.profile, args.packsquash, args.require_packsquash)
            print("OK:", len(report["packs"]), "packs, perfil", report["profile"])
            for pack in report["packs"]:
                print(" ", pack["filename"], round(pack["bytes"] / 1048576, 2), "MiB", pack["sha1"])
            for note in report["notes"]:
                print(" Nota:", note)
        elif args.command == "verify":
            print("OK:", len(verify(args.directory)["packs"]), "packs verificados")
        elif args.command == "rollback-r2":
            rollback_r2(args.directory, args.prefix)
        else:
            publish_r2(args.directory, args.prefix)
        return 0
    except (PackError, OSError, KeyError, ValueError) as exc:
        print("VSPack: " + str(exc), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
