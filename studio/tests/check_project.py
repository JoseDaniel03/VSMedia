from pathlib import Path
import tinycss2
root=Path(__file__).resolve().parents[1]
files=['Cargo.toml','Trunk.toml','index.html','start.sh','src/app.rs','src/main.rs','src/media.rs','src/export.rs','src/recording.rs','src/teleprompter.rs','src/studio_api.rs','src/api_contract.rs','assets/studio.css','docs/API.md','backend/Cargo.toml','backend/src/main.rs']
assert all((root/f).is_file() for f in files)
app=(root/'src/app.rs').read_text()
assert all(s in app for s in ['"Home"','"VSAudio"','"VSMix"','"VSRadio"','"LECTOR DE GUION','"CARPETAS"'])
assert not any(s in app for s in ['GlobalMenu','Guardar distribución','Enfocar','Layout','logo.svg','theme-dark'])
assert 'local_storage' in (root/'src/media.rs').read_text()
assert 'decode_audio_data_with_array_buffer' in (root/'src/export.rs').read_text()
assert 'MediaRecorder' in (root/'src/recording.rs').read_text()
css=(root/'assets/studio.css').read_text()
assert '.app-shell{height:100dvh' in css
assert not any(t.type=='error' for t in tinycss2.parse_stylesheet(css))
print('OK: archivos, estructura, rutas, CSS y regresiones de diseño')
