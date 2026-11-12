#!/usr/bin/env bash
# Build the same pinned MIT browser Vorbis encoder used by GitHub Actions.
# Heavy WAV/mix-to-OGG conversion runs in the player's admin browser.
set -euo pipefail
cd "$(dirname "$0")/.."
if ! command -v npm >/dev/null; then echo "Necesitás npm para preparar el codificador OGG." >&2; exit 1; fi
if [[ -s studio/assets/vs-ogg-vorbis.js ]]; then exit 0; fi
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
cd "$tmp"
npm init -y >/dev/null
npm install --no-audit --no-fund --save-exact @audio/encode-ogg@1.3.1 esbuild@0.25.12
cat > entry.mjs <<'EOF'
import ogg from '@audio/encode-ogg';
window.VSOggVorbis = Object.freeze({
  async encodeBuffer(buffer, {quality=5}={}) {
    if (!buffer || !buffer.numberOfChannels || !buffer.length) throw Error('No hay audio para convertir.');
    const channels = Math.min(2, buffer.numberOfChannels);
    if (buffer.duration > 30*60) throw Error('Máximo 30 minutos por sonido para proteger la memoria.');
    const encoder = await ogg({sampleRate:buffer.sampleRate, channels, quality});
    const pieces = [];
    try {
      for(let offset=0;offset<buffer.length;offset+=32768) {
        const end=Math.min(buffer.length, offset+32768);
        const chunk=Array.from({length:channels},(_,i)=>buffer.getChannelData(i).subarray(offset,end));
        const data=encoder.encode(chunk);
        if(data?.length) pieces.push(data.slice());
        if(offset%262144===0) await new Promise(resolve=>setTimeout(resolve,0));
      }
      const tail=encoder.flush();
      if(tail?.length)pieces.push(tail.slice());
    } finally { encoder.free?.(); }
    const result=new Blob(pieces,{type:'audio/ogg'});
    if(result.size<64)throw Error('La codificación Vorbis no produjo audio válido.');
    return result;
  }
});
EOF
./node_modules/.bin/esbuild entry.mjs --bundle --format=iife --platform=browser --target=es2020 \
  --outfile=vs-ogg-vorbis.js --minify --legal-comments=linked
test -s vs-ogg-vorbis.js
cp vs-ogg-vorbis.js "$OLDPWD/studio/assets/vs-ogg-vorbis.js"
if [[ -f node_modules/@audio/encode-ogg/LICENSE ]]; then
  cp node_modules/@audio/encode-ogg/LICENSE "$OLDPWD/studio/assets/LICENSE-encode-ogg.txt"
fi
