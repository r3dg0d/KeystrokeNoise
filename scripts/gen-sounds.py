#!/usr/bin/env python3
"""Synthesize default KeystrokeNoise WAV samples into assets/."""
import math, random, struct, wave
from pathlib import Path

SR = 44100

def clamp(x):
    return max(-1.0, min(1.0, x))

def write_wav(path, samples):
    with wave.open(str(path), "w") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(b"".join(struct.pack("<h", int(clamp(s) * 32767)) for s in samples))

def env(t, attack, decay):
    if t < attack:
        return t / attack
    return math.exp(-(t - attack) / decay)

def synth(kind):
    rnd = random.Random(hash(kind) & 0xFFFFFFFF)
    params = {
        "normal": (0.055, 420.0, 0.55, 0.35),
        "space": (0.085, 180.0, 0.45, 0.7),
        "enter": (0.095, 140.0, 0.4, 0.85),
        "modifier": (0.065, 280.0, 0.5, 0.45),
    }
    dur, f0, noise_amt, thock = params[kind]
    n = int(SR * dur)
    out = []
    for i in range(n):
        t = i / SR
        noise = rnd.random() * 2 - 1
        click = noise * env(t, 0.0008, 0.012) * noise_amt
        body = math.sin(2 * math.pi * f0 * t) * env(t, 0.0015, 0.028) * thock
        body += 0.35 * math.sin(2 * math.pi * (f0 * 0.5) * t) * env(t, 0.002, 0.04) * thock
        shell = 0.18 * math.sin(2 * math.pi * (f0 * 2.3) * t) * env(t, 0.001, 0.02)
        s = (click + body + shell) * 0.9
        if t > dur - 0.01:
            s *= (dur - t) / 0.01
        out.append(s)
    return out

def main():
    root = Path(__file__).resolve().parents[1] / "assets"
    root.mkdir(exist_ok=True)
    for kind in ["normal", "space", "enter", "modifier"]:
        write_wav(root / f"{kind}.wav", synth(kind))
        print("wrote", root / f"{kind}.wav")

if __name__ == "__main__":
    main()
