#!/usr/bin/env python3
"""Safely install a complete VSMedia Studio v11 ZIP into the repository."""
import os
import shutil
import stat
import sys
import tempfile
import zipfile
from pathlib import Path, PurePosixPath

REQUIRED = ('index.html', 'serve.mjs', 'start.sh', 'modules/vspack/vs-model-studio/dist/bundle.js')
def install(archive, dest):
    archive = Path(archive).expanduser().resolve()
    dest = Path(dest).resolve()
    if dest.is_symlink() or not zipfile.is_zipfile(archive):
        raise ValueError('Archivo ZIP o destino inválido')
    with zipfile.ZipFile(archive) as source:
        entries = [info for info in source.infolist() if not info.is_dir()]
        if not entries or len(entries) > 6000 or sum(i.file_size for i in entries) > 320*1024*1024:
            raise ValueError('Cantidad o tamaño de archivos fuera de límite')
        names = [PurePosixPath(i.filename).parts for i in entries]
        prefix = names[0][0] if all(len(p)>1 and p[0]==names[0][0] for p in names) else None
        paths = []
        seen = set()
        for item, pieces in zip(entries, names):
            if item.filename.startswith('/') or '\\' in item.filename or any(p in ('', '.', '..') for p in pieces):
                raise ValueError('Ruta peligrosa en el ZIP')
            if item.file_size > 96*1024*1024 or stat.S_IFMT(item.external_attr >> 16) == stat.S_IFLNK:
                raise ValueError('Archivo excesivo o enlace simbólico')
            rel = PurePosixPath(*pieces[1:]) if prefix else PurePosixPath(*pieces)
            key = str(rel).casefold()
            if not rel.parts or key in seen:
                raise ValueError('Ruta duplicada o vacía')
            seen.add(key)
            paths.append((item, rel))
        if any(name.casefold() not in seen for name in REQUIRED):
            raise ValueError('No es un VSMedia Studio v11 completo')
        dest.parent.mkdir(parents=True, exist_ok=True)
        tmp = Path(tempfile.mkdtemp(prefix='.vsmedia-import-', dir=dest.parent))
        try:
            for item, rel in paths:
                out = tmp.joinpath(*rel.parts)
                out.parent.mkdir(parents=True, exist_ok=True)
                with source.open(item) as src, out.open('wb') as dst:
                    shutil.copyfileobj(src, dst, 1024*1024)
            os.chmod(tmp/'start.sh', 0o755)
            if (tmp/'start').exists(): os.chmod(tmp/'start', 0o755)
            backup = None
            if dest.exists():
                backup = Path(tempfile.mkdtemp(prefix='.vsmedia-web-backup-', dir=dest.parent))
                backup.rmdir()
                dest.rename(backup)
            try: tmp.rename(dest)
            except Exception:
                if backup: backup.rename(dest)
                raise
            if backup: print(f'[VSMedia] Web anterior respaldada: {backup}')
            print(f'[VSMedia] Web v11 importada: {len(paths)} archivos. Iniciá con: vsmedia web')
        finally:
            if tmp.exists(): shutil.rmtree(tmp)
if __name__ == '__main__':
    if len(sys.argv) != 3:
        print('Uso: import-web-v11.py archivo.zip directorio', file=sys.stderr)
        sys.exit(2)
    try: install(sys.argv[1], sys.argv[2])
    except (OSError, ValueError, zipfile.BadZipFile) as error:
        print(f'[VSMedia] {error}', file=sys.stderr)
        sys.exit(1)
