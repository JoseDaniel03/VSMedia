/* Original VSRadio worklet: transport-only PCM16 mono 48 kHz, 20 ms chunks.
 * No recording or persistent storage. WebAudio graph is owned by radio-dj.js.
 */
class VSRadioPcm20 extends AudioWorkletProcessor {
  constructor() {
    super();
    this.chunk = new Int16Array(960);
    this.position = 0;
  }
  process(inputs, outputs) {
    const samples = inputs[0]?.[0];
    if (samples) {
      for (let i=0; i<samples.length; i++) {
        const x=Math.max(-1,Math.min(1,samples[i]));
        this.chunk[this.position++] = x < 0 ? Math.round(x*32768) : Math.round(x*32767);
        if (this.position === 960) {
          this.port.postMessage(this.chunk, [this.chunk.buffer]);
          this.chunk=new Int16Array(960);
          this.position=0;
        }
      }
    }
    const out = outputs[0];
    if (out) for (const channel of out) channel.fill(0);
    return true;
  }
}
registerProcessor("vsradio-pcm20", VSRadioPcm20);
