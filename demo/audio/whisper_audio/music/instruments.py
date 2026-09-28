"""Synthesised instruments for the score. Every voice returns a mono clip.

Nothing here knows about the Film's timing; `layers/music.py` places the notes.
Randomness always comes from a caller-supplied generator so rebuilds are identical.
"""

from __future__ import annotations

import numpy as np

from ..dsp import attack_release, exp_decay, fft_filter, midi_hz, seconds


def karplus_strong(
    note: float,
    length: float,
    sr: int,
    rng: np.random.Generator,
    brightness: float = 0.5,
    decay: float = 0.996,
) -> np.ndarray:
    """A plucked string: a noise burst circulating in a delay line with a two-tap averaging low-pass.

    `brightness` (0..1) shapes the initial burst (lower = softer, more like a thumb pluck);
    `decay` is the loop gain per period. Computed one period at a time, so a note costs
    about `length * f` numpy operations rather than a Python loop per sample.
    """
    period = max(2, int(round(sr / midi_hz(note))))
    n = seconds(length, sr)
    burst = rng.uniform(-1.0, 1.0, period)
    burst = brightness * burst + (1.0 - brightness) * np.convolve(burst, [0.5, 0.5], mode="same")
    burst -= np.mean(burst)

    # buf[0] is a leading zero so that buf[i - period - 1] exists for the first loop.
    buf = np.zeros(n + 1)
    buf[1 : 1 + min(period, n)] = burst[: min(period, n)]
    for k in range(period + 1, n + 1, period):
        end = min(k + period, n + 1)
        buf[k:end] = decay * 0.5 * (buf[k - period : end - period] + buf[k - period - 1 : end - period - 1])
    out = buf[1:]
    return out * attack_release(n, seconds(0.002, sr), seconds(0.08, sr))


# Kalimba tine partials: (frequency ratio, amplitude, decay seconds). The upper
# partials are inharmonic and die away fast, which gives the bell-like attack.
_KALIMBA_PARTIALS = ((1.0, 1.0, 1.1), (2.76, 0.32, 0.22), (5.4, 0.14, 0.07))


def kalimba(note: float, length: float, sr: int, rng: np.random.Generator, ring: float = 1.0) -> np.ndarray:
    """A kalimba tine: a few decaying sine partials at inharmonic ratios plus a tiny thumb click.

    `ring` scales every partial's decay time (above 1 lets a closing note hang on).
    """
    n = seconds(length, sr)
    t = np.arange(n) / sr
    f = midi_hz(note)
    out = np.zeros(n)
    for ratio, amp, tau in _KALIMBA_PARTIALS:
        out += amp * np.sin(2 * np.pi * f * ratio * t + rng.uniform(0, 2 * np.pi)) * np.exp(-t / (tau * ring))
    click = rng.standard_normal(n) * exp_decay(n, 0.0015 * sr) * 0.12
    return (out + click) * attack_release(n, seconds(0.001, sr), seconds(0.06, sr))


# Round bass partials: (harmonic, amplitude). Almost a pure sine; the faint 2nd and 3rd
# harmonics let the line be heard on small speakers without adding boom.
_BASS_HARMONICS = ((1, 1.0), (2, 0.18), (3, 0.06))


def round_bass(note: float, length: float, sr: int, decay: float = 1.6) -> np.ndarray:
    """A round, sine-based bass note: a soft attack, a slow decay, and a little harmonic colour.

    The upper harmonics fade faster than the fundamental, so the note warms as it sustains.
    """
    n = seconds(length, sr)
    t = np.arange(n) / sr
    f = midi_hz(note)
    out = np.zeros(n)
    for k, amp in _BASS_HARMONICS:
        if f * k < sr / 2:
            out += amp * np.sin(2 * np.pi * f * k * t) * np.exp(-t * k / decay)
    return out * attack_release(n, seconds(0.012, sr), seconds(0.12, sr))


def soft_kick(sr: int) -> np.ndarray:
    """A felt-beater kick: a short pitch-dropping sine, no click, so it thumps rather than knocks."""
    n = seconds(0.4, sr)
    t = np.arange(n) / sr
    freq = 52.0 + 55.0 * np.exp(-t / 0.03)
    body = np.sin(2 * np.pi * np.cumsum(freq) / sr) * exp_decay(n, 0.13 * sr)
    return body * attack_release(n, seconds(0.003, sr), seconds(0.06, sr))


def brush_tap(sr: int, rng: np.random.Generator, length: float = 0.3) -> np.ndarray:
    """A brush slapped on the snare: band-passed noise with a soft attack and a faint drum-body tone."""
    n = seconds(length, sr)
    t = np.arange(n) / sr
    noise = fft_filter(rng.standard_normal(n), sr, lowpass=4200, highpass=1000)
    noise /= np.max(np.abs(noise)) + 1e-12
    env = attack_release(n, seconds(0.004, sr), seconds(0.05, sr)) * exp_decay(n, 0.07 * sr)
    body = 0.25 * np.sin(2 * np.pi * 190 * t) * exp_decay(n, 0.025 * sr)
    return noise * env + body


def brush_tick(sr: int, rng: np.random.Generator) -> np.ndarray:
    """The brush tip on the off-beat: a short, soft, high noise tick (a gentle hi-hat stand-in)."""
    n = seconds(0.06, sr)
    noise = fft_filter(rng.standard_normal(n), sr, lowpass=7000, highpass=3500)
    noise /= np.max(np.abs(noise)) + 1e-12
    return noise * exp_decay(n, 0.012 * sr) * attack_release(n, seconds(0.002, sr), seconds(0.01, sr))


def brush_sweep(length: float, sr: int, rng: np.random.Generator) -> np.ndarray:
    """A brush circling on the snare head: band-passed noise swelling and fading over `length` seconds."""
    n = seconds(length, sr)
    noise = fft_filter(rng.standard_normal(n), sr, lowpass=3800, highpass=700)
    noise /= np.std(noise) + 1e-12
    swell = np.sin(np.pi * np.arange(n) / n) ** 1.5
    return noise * swell


def pad(notes: list[float], length: float, sr: int, rng: np.random.Generator) -> np.ndarray:
    """A soft stereo string pad: detuned additive saw-ish voices with a slow swell and release.

    Returns shape (n, 2); left and right voices are detuned in opposite directions for width.
    """
    n = seconds(length, sr)
    t = np.arange(n) / sr
    out = np.zeros((n, 2))
    for note in notes:
        f = midi_hz(note)
        for channel, cents in ((0, -7.0), (1, 7.0)):
            fd = f * 2.0 ** ((cents + rng.normal(0, 1.5)) / 1200)
            phase = rng.uniform(0, 2 * np.pi)
            voice = sum(np.sin(2 * np.pi * fd * k * t + k * phase) / k**1.6 for k in range(1, 6) if fd * k < 5000)
            out[:, channel] += voice
    swell = attack_release(n, seconds(min(1.4, length / 3), sr), seconds(min(1.6, length / 2), sr))
    breathe = 1.0 + 0.08 * np.sin(2 * np.pi * 0.23 * t + rng.uniform(0, 2 * np.pi))
    return out * (swell * breathe)[:, None] / max(len(notes), 1)


def paper_rustle(n: int, sr: int, rng: np.random.Generator, rate: float = 0.5) -> np.ndarray:
    """Stereo paper handling: sparse crinkly bursts of band-passed noise, `rate` bursts per second on average.

    Each burst is a cluster of tiny crackles under a slow swell, so it reads as a sheet
    being moved rather than as hiss.
    """
    out = np.zeros((n, 2))
    count = rng.poisson(rate * n / sr)
    for at in np.sort(rng.integers(0, max(n - sr, 1), count)):
        length = seconds(rng.uniform(0.25, 0.7), sr)
        grains = np.zeros(length)
        hits = rng.integers(0, length, int(length / sr * rng.uniform(60, 160)))
        np.add.at(grains, hits, rng.uniform(-1.0, 1.0, hits.shape[0]))
        grains += 0.25 * rng.standard_normal(length)
        swell = np.sin(np.pi * np.arange(length) / length) ** 2
        burst = grains * swell * rng.uniform(0.4, 1.0)
        pan = rng.uniform(-0.6, 0.6)
        angle = (pan + 1.0) * np.pi / 4.0
        out[at : at + length, 0] += burst * np.cos(angle)
        out[at : at + length, 1] += burst * np.sin(angle)
    return fft_filter(out, sr, lowpass=7000, highpass=1500)


def room(
    x: np.ndarray,
    sr: int,
    rng: np.random.Generator,
    decay: float = 1.4,
    predelay: float = 0.018,
    tone: float = 3500.0,
) -> np.ndarray:
    """A small warm room: stereo convolution with decorrelated, exponentially decaying, darkened noise.

    `tone` is the low-pass on the tail (lower is softer). `x` has shape (n, 2); the wet
    signal has the same shape (the tail past the end is dropped).
    """
    n = x.shape[0]
    ir_n = seconds(decay * 1.6, sr)
    lead = seconds(predelay, sr)
    ir = rng.standard_normal((ir_n, 2)) * exp_decay(ir_n, decay / 6.9 * sr)[:, None]
    ir = fft_filter(ir, sr, lowpass=tone, highpass=150)
    ir[:lead] = 0.0
    ir /= np.sqrt(np.sum(ir**2, axis=0, keepdims=True)) + 1e-12
    size = 1 << int(np.ceil(np.log2(n + ir_n)))
    wet = np.fft.irfft(np.fft.rfft(x, size, axis=0) * np.fft.rfft(ir, size, axis=0), size, axis=0)
    return wet[:n]
