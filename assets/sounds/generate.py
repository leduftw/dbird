#!/usr/bin/env python3
"""Synthesize dbird's five sound effects from scratch.

Every effect is composed programmatically below — plain oscillators, filtered
noise, and envelopes — so the repository contains no third-party audio.
Regeneration is deterministic (seeded RNG). Requires only the Python standard
library plus `oggenc` (vorbis-tools) or `ffmpeg` on PATH for the final Ogg
Vorbis encode.

Usage:  python3 assets/sounds/generate.py
Writes the five sfx_*.ogg files next to this script.
"""

import math
import random
import shutil
import struct
import subprocess
import sys
import tempfile
import wave
from pathlib import Path

SAMPLE_RATE = 44_100
OUT_DIR = Path(__file__).resolve().parent


# --- small synthesis toolkit -------------------------------------------------


def seconds(n: float) -> int:
    return int(n * SAMPLE_RATE)


def silence(duration: float) -> list[float]:
    return [0.0] * seconds(duration)


def decay_env(i: int, attack: float, tau: float) -> float:
    """Fast attack, exponential decay; `tau` in seconds."""
    t = i / SAMPLE_RATE
    if t < attack:
        return t / attack
    return math.exp(-(t - attack) / tau)


def exp_sweep(t: float, duration: float, start: float, end: float) -> float:
    """Exponential glide from `start` to `end` Hz, clamped after `duration`."""
    x = min(max(t / duration, 0.0), 1.0)
    return start * (end / start) ** x


class BandPass:
    """RBJ constant-skirt band-pass biquad with a per-sample center frequency."""

    def __init__(self, q: float) -> None:
        self.q = q
        self.x1 = self.x2 = self.y1 = self.y2 = 0.0

    def process(self, x: float, center_hz: float) -> float:
        w0 = 2.0 * math.pi * center_hz / SAMPLE_RATE
        alpha = math.sin(w0) / (2.0 * self.q)
        a0 = 1.0 + alpha
        y = (
            (alpha / a0) * (x - self.x2)
            - (-2.0 * math.cos(w0) / a0) * self.y1
            - ((1.0 - alpha) / a0) * self.y2
        )
        self.x2, self.x1 = self.x1, x
        self.y2, self.y1 = self.y1, y
        return y


class OnePoleLowPass:
    def __init__(self) -> None:
        self.y = 0.0

    def process(self, x: float, cutoff_hz: float) -> float:
        a = 1.0 - math.exp(-2.0 * math.pi * cutoff_hz / SAMPLE_RATE)
        self.y += a * (x - self.y)
        return self.y


def drive(x: float, gain: float) -> float:
    """Soft saturation for punch."""
    return math.tanh(gain * x) / math.tanh(gain)


def mix_into(dest: list[float], src: list[float], at: float = 0.0, gain: float = 1.0) -> None:
    offset = seconds(at)
    for i, s in enumerate(src):
        j = offset + i
        if j < len(dest):
            dest[j] += gain * s


def normalized(channels: list[list[float]], peak: float) -> list[list[float]]:
    top = max(abs(s) for ch in channels for s in ch)
    scale = peak / top if top else 0.0
    return [[s * scale for s in ch] for ch in channels]


def write_wav(path: Path, channels: list[list[float]]) -> None:
    frames = len(channels[0])
    with wave.open(str(path), "wb") as w:
        w.setnchannels(len(channels))
        w.setsampwidth(2)
        w.setframerate(SAMPLE_RATE)
        data = bytearray()
        for i in range(frames):
            for ch in channels:
                data += struct.pack("<h", int(max(-1.0, min(1.0, ch[i])) * 32_767))
        w.writeframes(bytes(data))


# --- the five effects --------------------------------------------------------
# Peak levels roughly follow the loudness balance the game already had:
# strong flap/point/hit, softer swoosh, quiet die.


def wing() -> list[list[float]]:
    """A short airy flick with a small upward chirp — one wing beat. Mono."""
    out = silence(0.32)
    rng = random.Random(101)
    bp = BandPass(q=1.0)
    for i in range(seconds(0.16)):
        t = i / SAMPLE_RATE
        center = exp_sweep(t, 0.09, 800.0, 2_600.0)
        out[i] += bp.process(rng.uniform(-1.0, 1.0), center) * decay_env(i, 0.004, 0.035)
    phase = 0.0
    for i in range(seconds(0.12)):
        t = i / SAMPLE_RATE
        phase += 2.0 * math.pi * exp_sweep(t, 0.07, 480.0, 980.0) / SAMPLE_RATE
        out[i] += 0.5 * math.sin(phase) * decay_env(i, 0.003, 0.030)
    return normalized([out], peak=0.93)


def point() -> list[list[float]]:
    """A bright two-note chime, C6 up to G6, with a ringing bell tail. Stereo."""

    def bell(freq: float, tau: float, detune_cents: float) -> list[float]:
        note = silence(0.85)
        partials = [(1.0, 1.0, tau), (2.0, 0.35, tau * 0.5), (3.0, 0.12, tau * 0.3)]
        for voice_cents in (0.0, 4.0 + detune_cents):
            f = freq * 2.0 ** (voice_cents / 1_200.0)
            for ratio, amp, ptau in partials:
                phase = 0.0
                for i in range(len(note)):
                    phase += 2.0 * math.pi * f * ratio / SAMPLE_RATE
                    note[i] += 0.5 * amp * math.sin(phase) * decay_env(i, 0.002, ptau)
        return note

    channels = []
    for detune in (0.0, -2.5):  # tiny left/right detune for width
        ch = silence(1.0)
        mix_into(ch, bell(1_046.5, 0.09, detune), at=0.0, gain=0.55)
        mix_into(ch, bell(1_568.0, 0.30, detune), at=0.10, gain=1.0)
        channels.append(ch)
    return normalized(channels, peak=0.95)


def hit() -> list[list[float]]:
    """A punchy collision thump: dropping body plus a clipped noise crunch. Stereo."""
    body = silence(0.42)
    phase = 0.0
    for i in range(len(body)):
        t = i / SAMPLE_RATE
        phase += 2.0 * math.pi * exp_sweep(t, 0.20, 150.0, 52.0) / SAMPLE_RATE
        body[i] = drive(math.sin(phase) * decay_env(i, 0.002, 0.10), 2.5)

    channels = []
    for seed in (211, 212):
        rng = random.Random(seed)
        lp = OnePoleLowPass()
        rumble = OnePoleLowPass()
        ch = silence(0.545)
        for i in range(seconds(0.30)):
            t = i / SAMPLE_RATE
            noise = rng.uniform(-1.0, 1.0)
            crunch = lp.process(noise, exp_sweep(t, 0.15, 3_500.0, 250.0))
            ch[i] += 0.8 * drive(crunch * decay_env(i, 0.001, 0.05), 3.0)
            ch[i] += 0.25 * rumble.process(noise, 150.0) * decay_env(i, 0.02, 0.15)
        mix_into(ch, body)
        channels.append(ch)
    return normalized(channels, peak=0.91)


def die() -> list[list[float]]:
    """A soft buzzy tone falling from 640 Hz to 135 Hz — the tumble down. Stereo."""
    channels = []
    for detune_cents in (0.0, 2.0):
        ch = silence(0.75)
        phase = 0.0
        for i in range(seconds(0.62)):
            t = i / SAMPLE_RATE
            vibrato = 1.0 + 0.015 * min(t / 0.3, 1.0) * math.sin(2.0 * math.pi * 6.5 * t)
            f = exp_sweep(t, 0.55, 640.0, 135.0) * vibrato * 2.0 ** (detune_cents / 1_200.0)
            phase += 2.0 * math.pi * f / SAMPLE_RATE
            tone = (
                math.sin(phase)
                + math.sin(3.0 * phase) / 3.0
                + math.sin(5.0 * phase) / 5.0
            )
            env = min(t / 0.01, 1.0) if t < 0.45 else math.exp(-(t - 0.45) / 0.06)
            ch[i] = drive(tone * env, 1.5)
        channels.append(ch)
    return normalized(channels, peak=0.25)


def swooshing() -> list[list[float]]:
    """A broadband whoosh that rises then falls away — screen transitions. Stereo."""
    channels = []
    for seed in (301, 302):
        rng = random.Random(seed)
        bp = BandPass(q=0.9)
        ch = silence(1.6)
        for i in range(seconds(1.0)):
            t = i / SAMPLE_RATE
            if t < 0.5:
                center = exp_sweep(t, 0.5, 350.0, 2_900.0)
            else:
                center = exp_sweep(t - 0.5, 0.35, 2_900.0, 900.0)
            attack = min(t / 0.15, 1.0)
            release = 1.0 if t < 0.55 else math.exp(-(t - 0.55) / 0.18)
            ch[i] = bp.process(rng.uniform(-1.0, 1.0), center) * attack * release
        channels.append(ch)
    return normalized(channels, peak=0.74)


# --- output ------------------------------------------------------------------


def encode(wav: Path, ogg: Path) -> None:
    if shutil.which("oggenc"):
        subprocess.run(["oggenc", "-Q", "-q", "5", "-o", str(ogg), str(wav)], check=True)
    elif shutil.which("ffmpeg"):
        subprocess.run(
            ["ffmpeg", "-y", "-v", "error", "-i", str(wav),
             "-c:a", "vorbis", "-strict", "experimental", str(ogg)],
            check=True,
        )
    else:
        sys.exit("error: need oggenc (vorbis-tools) or ffmpeg to encode Ogg Vorbis")


def main() -> int:
    effects = {
        "sfx_wing": wing,
        "sfx_point": point,
        "sfx_hit": hit,
        "sfx_die": die,
        "sfx_swooshing": swooshing,
    }
    with tempfile.TemporaryDirectory() as tmp:
        for name, build in effects.items():
            wav = Path(tmp) / f"{name}.wav"
            write_wav(wav, build())
            encode(wav, OUT_DIR / f"{name}.ogg")
            print(f"wrote {OUT_DIR / f'{name}.ogg'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
