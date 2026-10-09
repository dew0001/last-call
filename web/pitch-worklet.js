// A small real-time pitch shifter for drunk voices (plan sections 4.6 and 8).
//
// Two read taps move through a delay line at a speed set by `pitch`, half a
// window apart, and cross-fade with sin^2 weights that always sum to 1. A tap
// whose delay grows by (1 - pitch) samples per sample plays the input back at
// `pitch` times its speed, so the voice sounds lower without slowing down.
// At pitch 1 the input passes straight through.

const WINDOW = 2048; // samples per grain, about 43 ms at 48 kHz

class PitchShift extends AudioWorkletProcessor {
  static get parameterDescriptors() {
    return [{ name: 'pitch', defaultValue: 1, minValue: 0.5, maxValue: 2, automationRate: 'k-rate' }];
  }

  constructor() {
    super();
    this.size = WINDOW * 2;
    this.buf = new Float32Array(this.size);
    this.write = 0;
    this.phase = 0;
  }

  read(delay) {
    let pos = this.write - delay;
    while (pos < 0) pos += this.size;
    const i = Math.floor(pos);
    const frac = pos - i;
    const a = this.buf[i % this.size];
    const b = this.buf[(i + 1) % this.size];
    return a + (b - a) * frac;
  }

  process(inputs, outputs, params) {
    const input = inputs[0]?.[0];
    const out = outputs[0];
    if (!input) return true;
    const pitch = params.pitch[0];
    if (pitch === 1) {
      for (const ch of out) ch.set(input);
      return true;
    }
    const step = (1 - pitch) / WINDOW;
    const mono = out[0];
    for (let i = 0; i < input.length; i++) {
      this.buf[this.write] = input[i];
      this.phase = (((this.phase + step) % 1) + 1) % 1;
      const p2 = (this.phase + 0.5) % 1;
      const g1 = Math.sin(Math.PI * this.phase) ** 2;
      const g2 = Math.sin(Math.PI * p2) ** 2;
      mono[i] = this.read(this.phase * WINDOW + 1) * g1 + this.read(p2 * WINDOW + 1) * g2;
      this.write = (this.write + 1) % this.size;
    }
    for (let c = 1; c < out.length; c++) out[c].set(mono);
    return true;
  }
}

registerProcessor('pitch-shift', PitchShift);
