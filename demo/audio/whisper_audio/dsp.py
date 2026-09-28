"""Small DSP helpers shared by layers and sound effects."""

from __future__ import annotations

import numpy as np


def seconds(n: float, sr: int) -> int:
    return int(round(n * sr))


def exp_decay(length: int, tau_samples: float) -> np.ndarray:
    return np.exp(-np.arange(length) / max(tau_samples, 1.0))


def attack_release(length: int, attack: int, release: int) -> np.ndarray:
    env = np.ones(length)
    attack = min(attack, length)
    release = min(release, length - attack)
    if attack:
        env[:attack] = np.linspace(0.0, 1.0, attack, endpoint=False)
    if release:
        env[length - release :] = np.linspace(1.0, 0.0, release)
    return env


def fade(x: np.ndarray, sr: int, attack: float = 0.0, release: float = 0.0) -> np.ndarray:
    """`x` with a linear fade-in over `attack` seconds and fade-out over `release` seconds.

    A short attack avoids a click at the start of a clip; a release lets it end in silence.
    """
    return x * attack_release(x.shape[0], seconds(attack, sr), seconds(release, sr))


def normalise(x: np.ndarray, peak: float = 1.0) -> np.ndarray:
    """`x` scaled so its largest absolute sample is `peak` (an all-zero `x` is returned scaled by `peak`)."""
    current = float(np.max(np.abs(x))) if x.size else 0.0
    return peak * x / (current or 1.0)


def tape_saturate(x: np.ndarray, drive: float = 1.5, bias: float = 0.1) -> np.ndarray:
    """Tape-style saturation: a slightly asymmetric tanh curve that rounds off peaks.

    Unity gain for quiet material, so it only thickens the loud moments; the `bias`
    makes the curve lopsided, which adds the soft even harmonics of tape. The bias
    leaves a small DC offset under loud passages, so high-pass the result.
    """
    offset = np.tanh(drive * bias)
    return (np.tanh(drive * (x + bias)) - offset) / (drive * (1.0 - offset**2))


def place(buffer: np.ndarray, clip: np.ndarray, at_sample: int, pan: float = 0.0) -> None:
    """Add a mono `clip` into stereo `buffer` at `at_sample` with equal-power pan (-1..1)."""
    if at_sample >= buffer.shape[0]:
        return
    end = min(buffer.shape[0], at_sample + clip.shape[0])
    clip = clip[: end - at_sample]
    angle = (pan + 1.0) * np.pi / 4.0
    buffer[at_sample:end, 0] += clip * np.cos(angle)
    buffer[at_sample:end, 1] += clip * np.sin(angle)


def midi_hz(note: float) -> float:
    return 440.0 * 2.0 ** ((note - 69.0) / 12.0)


def fft_filter(
    x: np.ndarray,
    sr: int,
    lowpass: float | None = None,
    highpass: float | None = None,
    order: int = 2,
) -> np.ndarray:
    """Zero-phase Butterworth-shaped low/high-pass along axis 0, done in the frequency domain.

    Fast enough for whole-Film buffers with numpy alone. The filter is circular, so
    only use it on material whose ends are quiet (or where wrap-around is inaudible).
    """
    n = x.shape[0]
    spectrum = np.fft.rfft(x, axis=0)
    f = np.fft.rfftfreq(n, 1.0 / sr)
    gain = np.ones_like(f)
    if lowpass is not None:
        gain /= np.sqrt(1.0 + (f / lowpass) ** (2 * order))
    if highpass is not None:
        with np.errstate(divide="ignore"):
            gain /= np.sqrt(1.0 + (highpass / np.maximum(f, 1e-9)) ** (2 * order))
    if x.ndim > 1:
        gain = gain[:, None]
    return np.fft.irfft(spectrum * gain, n=n, axis=0)
