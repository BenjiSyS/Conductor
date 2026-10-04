// A short two-tone "ding", synthesized so no audio file ships with the app.
let ctx: AudioContext | null = null;

export function ding(volume = 0.18) {
  try {
    ctx ??= new AudioContext();
    const now = ctx.currentTime;
    for (const [freq, at] of [
      [1318.5, 0],
      [1760, 0.09],
    ] as const) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = 'sine';
      osc.frequency.value = freq;
      gain.gain.setValueAtTime(0, now + at);
      gain.gain.linearRampToValueAtTime(volume, now + at + 0.01);
      gain.gain.exponentialRampToValueAtTime(0.0001, now + at + 0.6);
      osc.connect(gain).connect(ctx.destination);
      osc.start(now + at);
      osc.stop(now + at + 0.65);
    }
  } catch {
    // Audio unavailable (e.g. no output device): stay silent.
  }
}
