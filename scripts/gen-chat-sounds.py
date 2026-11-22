#!/usr/bin/env python3
"""Звуки чата Peerage: приход в открытый чат и отправка (notifications.md → «Звуки в приложении»).

Синтез, а не сэмплы: звук — наша работа под GPL, без чужих файлов и лицензий. Чтобы не
звучало «электронно», каждый звук — физическая модель: затухающие моды настоящего тела
(здесь — капля) со своими неровными обертонами, шумовой
удар в начале (палочка, щипок) и маленькая комната — сухой синтез без неё режет ухо.

Запуск: python scripts/gen-chat-sounds.py  → android/app/src/main/res/raw/chat_in.wav и
chat_out.wav; `--demo <каталог>` — туда же и chat-demo.wav (отправка, приход, пачка).
"""

import sys
import wave
from pathlib import Path

import numpy as np

SR = 48000
RNG = np.random.default_rng(7)  # звуки воспроизводимы от запуска к запуску


def t_axis(sec):
    return np.arange(int(SR * sec)) / SR


def lowpass_fft(x, fc, slope=2.0):
    """Плавный фильтр нижних частот в частотной области (без звона прямоугольного среза)."""
    X = np.fft.rfft(x)
    f = np.fft.rfftfreq(len(x), 1 / SR)
    return np.fft.irfft(X / np.sqrt(1 + (f / fc) ** (2 * slope)), len(x))


def bandpass_fft(x, lo, hi):
    return lowpass_fft(x, hi) - lowpass_fft(x, lo)


def strike(sec, fc, tau):
    """Шумовой удар — касание палочкой, щипок, капля о воду: короткий окрашенный шум."""
    t = t_axis(sec)
    n = RNG.standard_normal(len(t)) * np.exp(-t / tau)
    return lowpass_fft(n, fc)


def room(x, tau=0.07, wet=0.16, predelay=0.006):
    """Маленькая комната: свёртка с затухающим тёмным шумом. Сухой синтез звучит «в голове»."""
    t = t_axis(tau * 6)
    ir = RNG.standard_normal(len(t)) * np.exp(-t / tau)
    ir = lowpass_fft(ir, 3500)
    ir = np.concatenate([np.zeros(int(SR * predelay)), ir])
    ir /= np.sqrt(np.sum(ir**2))
    y = np.convolve(x, ir)[: len(x) + len(ir) // 2]
    out = np.zeros(len(y))
    out[: len(x)] += x
    return out + wet * y


def finish(x, peak_db):
    """Хвост — в ноль без щелчка, пик — на заданный уровень."""
    fade = int(SR * 0.012)
    x = x.copy()
    x[-fade:] *= np.linspace(1, 0, fade) ** 2
    nz = np.nonzero(np.abs(x) > 1e-4 * np.max(np.abs(x)))[0]
    x = x[: nz[-1] + 1] if len(nz) else x
    return x / np.max(np.abs(x)) * 10 ** (peak_db / 20)


# ── Звук: капля (выбрана 2026-11-22 из четырёх семей — капля, калимба, дерево, стекло) ──


def drop(f0, f1, sweep, tau, sec=0.35):
    """Капля в воду: резонанс воздушного пузырька растёт по высоте, пока он схлопывается
    (Миннарт). Плюс едва слышный всплеск в начале."""
    t = t_axis(sec)
    f = f0 * (f1 / f0) ** np.clip(t / sweep, 0, 1)  # экспоненциальный рост частоты
    ph = 2 * np.pi * np.cumsum(f) / SR
    body = (1 - np.exp(-t / 0.0008)) * np.exp(-t / tau) * np.sin(ph)
    body += 0.18 * (1 - np.exp(-t / 0.0008)) * np.exp(-t / (tau * 0.5)) * np.sin(2 * ph + 0.6)
    splash = np.zeros(len(t))
    s = bandpass_fft(strike(0.02, 9000, 0.0015), 2500, 9000)
    splash[: len(s)] = s
    return body + 0.12 * splash / np.max(np.abs(splash))


def incoming():
    return drop(620, 1350, 0.05, 0.045)


def outgoing():
    return drop(950, 2300, 0.03, 0.025, 0.25)


def write_wav(path, x):
    pcm = np.clip(x * 32767, -32768, 32767).astype("<i2")
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes(pcm.tobytes())


def main():
    """Без аргументов — в res/raw приложения; `--demo <каталог>` — ещё и пачка для прослушивания."""
    args = sys.argv[1:]
    demo = "--demo" in args
    args = [a for a in args if a != "--demo"]
    root = Path(__file__).resolve().parent.parent
    out = Path(args[0]) if args else root / "android/app/src/main/res/raw"
    out.mkdir(parents=True, exist_ok=True)
    # Порядок важен: ГСЧ общий, приход считается первым — так звуки не меняются от запуска к запуску.
    rin = finish(room(incoming()), -8.0)
    rout = finish(room(outgoing()), -11.0)  # отправка тише: её вызывает сам человек
    write_wav(out / "chat_in.wav", rin)
    write_wav(out / "chat_out.wav", rout)
    print(f"in {len(rin) / SR:.2f}s, out {len(rout) / SR:.2f}s -> {out}")
    if demo:
        gap = lambda s: np.zeros(int(SR * s))  # noqa: E731
        seq = [gap(0.3), rout, gap(0.9), rin, gap(0.9), rin, gap(0.35), rin, gap(0.4)]
        write_wav(out / "chat-demo.wav", np.concatenate(seq))


if __name__ == "__main__":
    main()
