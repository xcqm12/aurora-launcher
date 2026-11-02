"""Generate narration + background music for the Aurora promo video.

Pass 1 (narrations): python scripts/generate-promo-audio.py narrations
    -> synthesizes one mp3 per scene with edge-tts (zh-CN-XiaoxiaoNeural),
       writes scripts/promo-timeline.json so generate-promo.py can stretch
       each scene to fit its narration.

Pass 2 (mix): python scripts/generate-promo-audio.py mix <intro-duration>
    -> builds the full soundtrack (silence for the intro, then BGM + narration
       aligned to the scene offsets) as promo-audio.wav.
"""
from __future__ import annotations

import json
import subprocess
import sys
import wave
from pathlib import Path

import numpy as np

HERE = Path(__file__).parent
VOICE = "zh-CN-XiaoxiaoNeural"
RATE = "+12%"
INTRO_FALLBACK = 4.087
SCENE_PAD = 1.1  # seconds of breathing room after narration inside a scene
FADE = 0.45


def load_slides():
    import importlib.util

    spec = importlib.util.spec_from_file_location("gp", HERE / "generate-promo.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.SLIDES


def scene_offsets(slides):
    offsets = []
    cursor = 0.0
    for item in slides:
        offsets.append(cursor)
        cursor += item["duration"] - (FADE if item is not slides[-1] else 0.0)
    return offsets, cursor + FADE - slides[-1]["duration"] + slides[-1]["duration"]


def run(cmd: list[str]) -> None:
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        raise SystemExit(f"command failed: {' '.join(cmd)}\n{result.stderr[-600:]}")


def synth_narrations(slides) -> dict[str, float]:
    out_dir = HERE / "tts"
    out_dir.mkdir(exist_ok=True)
    lengths: dict[str, float] = {}
    for item in slides:
        name = item["name"]
        target = out_dir / f"{name}.mp3"
        if not target.exists() or target.stat().st_size < 1024:
            run(
                [
                    sys.executable,
                    "-m",
                    "edge_tts",
                    "--voice",
                    VOICE,
                    f"--rate={RATE}",
                    "--text",
                    item["narration"],
                    "--write-media",
                    str(target),
                ]
            )
        info = subprocess.run(
            [
                imageio_ffmpeg(),
                "-i",
                str(target),
                "-f",
                "null",
                "-",
            ],
            capture_output=True,
            text=True,
        )
        seconds = 0.0
        for line in info.stderr.splitlines():
            if "time=" in line:
                stamp = line.split("time=")[-1].split(" ")[0]
                parts = [float(part) for part in stamp.split(":")]
                seconds = parts[0] * 3600 + parts[1] * 60 + parts[2]
        lengths[name] = seconds
        print(f"  {name}: {seconds:.2f}s narration")
    return lengths


def imageio_ffmpeg() -> str:
    import imageio_ffmpeg

    return imageio_ffmpeg.get_ffmpeg_exe()


def decode_to_float(path: Path) -> tuple[np.ndarray, int]:
    target = path.with_suffix(".wav")
    run(
        [
            imageio_ffmpeg(),
            "-y",
            "-i",
            str(path),
            "-ar",
            "44100",
            "-ac",
            "2",
            "-f",
            "wav",
            str(target),
        ]
    )
    with wave.open(str(target), "rb") as handle:
        rate = handle.getframerate()
        channels = handle.getnchannels()
        frames = np.frombuffer(handle.readframes(handle.getnframes()), dtype=np.int16)
    audio = frames.reshape(-1, channels).astype(np.float32) / 32768.0
    return audio, rate


def write_wav(path: Path, audio: np.ndarray, rate: int) -> None:
    clipped = np.clip(audio, -1.0, 1.0)
    with wave.open(str(path), "wb") as handle:
        handle.setnchannels(2)
        handle.setsampwidth(2)
        handle.setframerate(rate)
        handle.writeframes((clipped * 32767).astype(np.int16).tobytes())


def resample_to(audio: np.ndarray, src_rate: int, dst_rate: int) -> np.ndarray:
    if src_rate == dst_rate:
        return audio
    count = int(len(audio) * dst_rate / src_rate)
    x_old = np.linspace(0, 1, len(audio))
    x_new = np.linspace(0, 1, count)
    return np.stack(
        [np.interp(x_new, x_old, audio[:, ch]) for ch in range(audio.shape[1])], axis=1
    )


def make_bgm(seconds: float, rate: int) -> np.ndarray:
    total = int(seconds * rate)
    t = np.arange(total) / rate
    # slow ambient pad: Am -> F -> C -> G, one chord every 4 seconds
    chords = [
        [110.0, 164.81, 220.0],
        [87.31, 130.81, 174.61],
        [130.81, 164.81, 196.0],
        [98.0, 146.83, 196.0],
    ]
    mix = np.zeros((total, 2), dtype=np.float32)
    chord_len = 4.0
    for index in range(int(np.ceil(seconds / chord_len))):
        start = int(index * chord_len * rate)
        end = min(int((index + 1) * chord_len * rate), total)
        length = end - start
        if length <= 0:
            break
        local_t = np.arange(length) / rate
        envelope = np.minimum(local_t / 0.8, 1.0) * np.minimum(
            (length - local_t) / 0.8, 1.0
        )
        for freq in chords[index % len(chords)]:
            wave_form = (
                np.sin(2 * np.pi * freq * local_t)
                + 0.35 * np.sin(2 * np.pi * freq * 2 * local_t + 0.7)
            )
            tremolo = 1 + 0.12 * np.sin(2 * np.pi * 0.25 * local_t)
            mix[start:end, 0] += (wave_form * envelope * tremolo * 0.05).astype(np.float32)
            mix[start:end, 1] += (
                wave_form * envelope * tremolo * 0.05 * np.roll(np.ones(length), 90)
            ).astype(np.float32)
    # gentle fade in/out
    fade_count = int(1.5 * rate)
    if total > fade_count * 2:
        mix[:fade_count] *= np.linspace(0, 1, fade_count)[:, None]
        mix[-fade_count:] *= np.linspace(1, 0, fade_count)[:, None]
    return mix


def cmd_narrations() -> None:
    slides = load_slides()
    lengths = synth_narrations(slides)
    timeline = {
        item["name"]: round(max(item["duration"], lengths[item["name"]] + SCENE_PAD), 2)
        for item in slides
    }
    (HERE / "promo-timeline.json").write_text(
        json.dumps(timeline, ensure_ascii=False, indent=1), encoding="utf-8"
    )
    total = sum(timeline.values()) - FADE * (len(slides) - 1)
    print("promo-timeline.json written; main video:", round(total, 2), "s")


def cmd_mix(intro_duration: float) -> None:
    slides = load_slides()
    timeline_file = HERE / "promo-timeline.json"
    overrides = json.loads(timeline_file.read_text(encoding="utf-8"))
    for item in slides:
        if item["name"] in overrides:
            item["duration"] = float(overrides[item["name"]])
    offsets, main_total = scene_offsets(slides)
    rate = 44100
    total = int((intro_duration + main_total + 1.5) * rate)
    mix = np.zeros((total, 2), dtype=np.float32)

    bgm = make_bgm(intro_duration + main_total + 3.0, rate)[:total]
    mix += bgm * 0.8

    for item, start in zip(slides, offsets):
        mp3 = HERE / "tts" / f"{item['name']}.mp3"
        audio, src_rate = decode_to_float(mp3)
        audio = resample_to(audio, src_rate, rate)
        at = int((intro_duration + start + 0.35) * rate)
        end = min(at + len(audio), total)
        segment = audio[: end - at]
        fade_count = min(int(0.08 * rate), len(segment) // 2)
        if fade_count:
            segment[:fade_count] *= np.linspace(0, 1, fade_count)[:, None]
            segment[-fade_count:] *= np.linspace(1, 0, fade_count)[:, None]
        mix[at:end] += segment * 1.15

    peak = float(np.max(np.abs(mix))) or 1.0
    if peak > 0.92:
        mix *= 0.92 / peak
    write_wav(HERE / "promo-audio.wav", mix, rate)
    print(
        f"promo-audio.wav written: {total / rate:.2f}s "
        f"(intro {intro_duration:.2f}s + main {main_total:.2f}s)"
    )


def main() -> None:
    mode = sys.argv[1] if len(sys.argv) > 1 else ""
    if mode == "narrations":
        cmd_narrations()
    elif mode == "mix":
        intro = float(sys.argv[2]) if len(sys.argv) > 2 else INTRO_FALLBACK
        cmd_mix(intro)
    else:
        raise SystemExit("usage: generate-promo-audio.py narrations | mix <intro-duration>")


if __name__ == "__main__":
    main()
