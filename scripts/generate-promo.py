"""Generate the Aurora Launcher promo video (1280x720 H.264, ~24s, no audio).

Frames are drawn with PIL and piped straight into the ffmpeg binary bundled
with imageio-ffmpeg, so the only requirements are Pillow + imageio-ffmpeg.

Usage: python scripts/generate-promo.py [output.mp4] [--fps 30]
"""
from __future__ import annotations

import math
import subprocess
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

W, H = 1280, 720
FPS = 30

BG_TOP = (14, 20, 36)
BG_BOTTOM = (6, 9, 17)
GLOW_A = (126, 167, 255)  # #7ea7ff
GLOW_B = (143, 240, 200)  # #8ff0c8
TEXT = (237, 243, 255)
MUTED = (137, 147, 170)
LINE = (38, 48, 74)
PANEL = (16, 22, 38)

FONT_DIR = Path(r"C:\Windows\Fonts")


def font(size: int, bold: bool = False):
    name = "msyhbd.ttc" if bold else "msyh.ttc"
    path = FONT_DIR / name
    if not path.exists():
        path = FONT_DIR / "msyh.ttc"
    return ImageFont.truetype(str(path), size)


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def ease_out(t: float) -> float:
    return 1 - (1 - t) ** 3


def clamp01(value: float) -> float:
    return max(0.0, min(1.0, value))


def background(frame_index: int, total: int) -> Image.Image:
    """Dark vertical gradient with two slowly drifting glow blobs."""
    image = Image.new("RGB", (W, H))
    top = Image.new("RGB", (1, H))
    for y in range(H):
        t = y / (H - 1)
        top.putpixel(
            (0, y),
            (
                int(lerp(BG_TOP[0], BG_BOTTOM[0], t)),
                int(lerp(BG_TOP[1], BG_BOTTOM[1], t)),
                int(lerp(BG_TOP[2], BG_BOTTOM[2], t)),
            ),
        )
    image = top.resize((W, H))

    phase = frame_index / max(total, 1)
    glow = Image.new("RGB", (W, H), (0, 0, 0))
    glow_draw = ImageDraw.Draw(glow)
    for (cx, cy, radius, color) in (
        (int(W * (0.78 + 0.06 * math.sin(phase * math.tau))), int(H * -0.10), 460, GLOW_A),
        (int(W * (0.16 + 0.05 * math.cos(phase * math.tau))), int(H * 1.06), 400, GLOW_B),
    ):
        glow_draw.ellipse(
            (cx - radius, cy - radius, cx + radius, cy + radius),
            fill=tuple(int(c * 0.30) for c in color),
        )
    glow = glow.filter(ImageFilter.GaussianBlur(160))
    image = Image.blend(image, Image.blend(image, glow, 0.55), 0.85)
    return image.convert("RGBA")


def fade(layer: Image.Image, alpha: float) -> Image.Image:
    if alpha >= 1:
        return layer
    if alpha <= 0:
        return Image.new("RGBA", layer.size, (0, 0, 0, 0))
    solid = Image.new("RGBA", layer.size, (0, 0, 0, 0))
    return Image.blend(solid, layer, clamp01(alpha))


def ease_slide(t: float, distance: float = 46):
    t = ease_out(clamp01(t))
    return int((1 - t) * distance), t


def text_layer(parts, anchor_x: int, anchor_y: int, align: str = "center") -> Image.Image:
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    widths = [draw.textlength(text, font=f) for text, f, _ in parts]
    total = sum(widths)
    x = anchor_x - total // 2 if align == "center" else anchor_x
    for (text, f, color), width in zip(parts, widths):
        draw.text((x, anchor_y), text, font=f, fill=color)
        x += width
    return layer


def panel_layer(x: int, y: int, w: int, h: int, radius: int = 16) -> Image.Image:
    layer = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    draw = ImageDraw.Draw(layer)
    draw.rounded_rectangle(
        (x, y, x + w, y + h), radius=radius, fill=(*PANEL, 232), outline=(*LINE, 255), width=2
    )
    return layer


def chip(draw: ImageDraw.ImageDraw, x: int, y: int, label: str, f, accent) -> int:
    width = int(draw.textlength(label, font=f)) + 36
    draw.rounded_rectangle((x, y, x + width, y + 44), radius=22, outline=(*accent, 220), width=2)
    draw.text((x + 18, y + 8), label, font=f, fill=(*TEXT, 255))
    return width + 16


SLIDES: list[dict] = []


def slide(duration: float):
    def register(func):
        SLIDES.append({"duration": duration, "render": func})
        return func

    return register


TITLE_F = font(76, bold=True)
SUB_F = font(26)
H2_F = font(46, bold=True)
BODY_F = font(23)
SMALL_F = font(18)
CHIP_F = font(20)
MONO_F = font(24)


@slide(3.6)
def hero(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.8)
    frame.alpha_composite(fade(text_layer([("AURORA ", TITLE_F, GLOW_A), ("LAUNCHER", TITLE_F, GLOW_B)], W // 2, 208 + appear), alpha))
    frame.alpha_composite(
        fade(
            text_layer(
                [("Windows 优先的 Minecraft 启动器 · Rust + Tauri", SUB_F, MUTED)],
                W // 2,
                320 + appear,
            ),
            alpha * 0.9,
        )
    )
    badge_w = 220
    frame.alpha_composite(
        fade(panel_layer(W // 2 - badge_w // 2, 396 + appear, badge_w, 52, 26), alpha),
    )
    frame.alpha_composite(
        fade(
            text_layer([("v0.5.4 · 2026-11-01", font(22, True), GLOW_B)], W // 2, 408 + appear),
            alpha,
        )
    )
    if local > 0.9:
        t2 = ease_out(clamp01((local - 0.9) / 0.6))
        frame.alpha_composite(
            fade(
                text_layer(
                    [
                        ("正版登录", BODY_F, TEXT),
                        ("  ·  ", BODY_F, MUTED),
                        ("加载器自动安装", BODY_F, TEXT),
                        ("  ·  ", BODY_F, MUTED),
                        ("镜像加速", BODY_F, TEXT),
                        ("  ·  ", BODY_F, MUTED),
                        ("EasyTier 联机", BODY_F, TEXT),
                    ],
                    W // 2,
                    int(lerp(520, 500, t2)),
                ),
                t2,
            )
        )


@slide(4.0)
def loaders(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("一键启动，加载器自动安装", H2_F, TEXT)], W // 2, 120 + appear), alpha))
    frame.alpha_composite(
        fade(text_layer([("创建实例时自动下载官方安装器并执行，失败自动补库重试（最多 3 次）", BODY_F, MUTED)], W // 2, 196 + appear), alpha * 0.95)
    )

    card_y = 268 + appear
    frame.alpha_composite(fade(panel_layer(140, card_y, W - 280, 352), alpha))
    frame.alpha_composite(fade(text_layer([("新建游戏实例", font(24, True), TEXT)], 190, card_y + 28, "left"), alpha))
    frame.alpha_composite(fade(text_layer([("Minecraft 版本  1.21.1", BODY_F, MUTED)], 190, card_y + 84, "left"), alpha))
    frame.alpha_composite(fade(text_layer([("加载器  Forge 47.4.0", BODY_F, MUTED)], 190, card_y + 126, "left"), alpha))

    # animated stage list
    stages = ["下载 MC 本体", "下载安装器", "运行安装器", "完成"]
    done_at = [0.9, 1.5, 2.2, 2.9]
    for index, stage in enumerate(stages):
        y = card_y + 178 + index * 30
        reached = local >= done_at[index]
        color = GLOW_B if reached else MUTED
        if reached and local < done_at[index] + 0.25:
            scale = ease_out(clamp01((local - done_at[index]) / 0.25))
        else:
            scale = 1.0 if reached else 0.6
        frame.alpha_composite(
            fade(text_layer([(f"√ {stage}", font(21, reached), (*color, int(255 * scale)))], 190, y, "left"), alpha)
        )

    x = 190
    y = card_y + 300
    for label in ("Forge", "NeoForge", "Fabric", "Legacy Fabric", "Cleanroom", "OptiFine", "LiteLoader"):
        width = chip(draw, x, y, label, CHIP_F, GLOW_A)
        x += width
        if x > W - 220:
            break


@slide(3.8)
def accounts(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("三种登录方式", H2_F, TEXT)], W // 2, 120 + appear), alpha))
    frame.alpha_composite(
        fade(text_layer([("令牌使用 Windows DPAPI 加密保存，不上传任何服务器", BODY_F, MUTED)], W // 2, 196 + appear), alpha * 0.95)
    )

    items = [
        ("微软正版登录", "内置已过审的官方 Client ID，开箱即用"),
        ("LittleSkin 外置登录", "OAuth 授权 / 账号密码，支持令牌刷新"),
        ("离线账户", "UUID 与正版规则一致，多实例互不影响"),
    ]
    for index, (title, detail) in enumerate(items):
        card_y = 268 + index * 118 + appear
        local_delay = local - index * 0.35
        if local_delay <= 0:
            continue
        card_alpha = ease_out(clamp01(local_delay / 0.5))
        frame.alpha_composite(fade(panel_layer(220, card_y, W - 440, 96), card_alpha))
        frame.alpha_composite(
            fade(text_layer([(f"√  {title}", font(26, True), GLOW_B)], 262, card_y + 16, "left"), card_alpha)
        )
        frame.alpha_composite(
            fade(text_layer([(detail, BODY_F, MUTED)], 262, card_y + 54, "left"), card_alpha)
        )


@slide(3.8)
def download(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("下载更快，看得见", H2_F, TEXT)], W // 2, 120 + appear), alpha))
    frame.alpha_composite(
        fade(text_layer([("官方 · BMCLAPI 镜像 · 自动探测最优下载源（缓存 10 分钟）", BODY_F, MUTED)], W // 2, 196 + appear), alpha * 0.95)
    )

    card_y = 276 + appear
    frame.alpha_composite(fade(panel_layer(170, card_y, W - 340, 230), alpha))
    frame.alpha_composite(
        fade(text_layer([("下载 MC 本体", font(23, True), TEXT)], 214, card_y + 26, "left"), alpha)
    )
    frame.alpha_composite(
        fade(text_layer([("libraries.minecraft.net", SMALL_F, MUTED)], 214, card_y + 66, "left"), alpha)
    )

    percent = ease_out(clamp01((local - 0.6) / 2.4))
    bar_x, bar_y, bar_w, bar_h = 214, card_y + 112, W - 428, 22
    frame.alpha_composite(fade(panel_layer(bar_x - 4, bar_y - 4, bar_w + 8, bar_h + 8, 12), alpha))
    draw.rounded_rectangle(
        (bar_x, bar_y, bar_x + int(bar_w * percent), bar_y + bar_h),
        radius=11,
        fill=(126, 167, 255, 235),
    )
    frame.alpha_composite(
        fade(
            text_layer(
                [(f"{int(percent * 100)}%", font(22, True), GLOW_B)],
                bar_x + bar_w + 70,
                bar_y - 6,
                "left",
            ),
            alpha,
        )
    )
    frame.alpha_composite(
        fade(
            text_layer(
                [("Modrinth / CurseForge 资源下载同样显示实时进度与 SHA 校验", BODY_F, MUTED)],
                W // 2,
                card_y + 172,
            ),
            alpha,
        )
    )


@slide(4.2)
def multiplayer(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("EasyTier 联机，房间码即开即联", H2_F, TEXT)], W // 2, 120 + appear), alpha))
    frame.alpha_composite(
        fade(text_layer([("与 PCL-CE 同源方案：虚拟局域网内直连或中继，无需公网 IP", BODY_F, MUTED)], W // 2, 196 + appear), alpha * 0.95)
    )

    card_y = 276 + appear
    frame.alpha_composite(fade(panel_layer(190, card_y, W - 380, 210), alpha))
    frame.alpha_composite(fade(text_layer([("创建房间", font(23, True), TEXT)], 236, card_y + 26, "left"), alpha))

    code = "aurora-3f9a2c#e81f77d4"
    typed = code[: int(clamp01((local - 0.8) / 1.4) * len(code))]
    caret = "▏" if int(local * 3) % 2 == 0 and len(typed) < len(code) else ""
    frame.alpha_composite(
        fade(text_layer([(typed + caret, MONO_F, GLOW_B)], 236, card_y + 74, "left"), alpha)
    )
    frame.alpha_composite(
        fade(text_layer([("朋友输入房间码 → 自动分配虚拟 IP  10.144.144.2", BODY_F, MUTED)], 236, card_y + 130, "left"), alpha)
    )
    if local > 2.6:
        connected = ease_out(clamp01((local - 2.6) / 0.5))
        frame.alpha_composite(
            fade(text_layer([("● 已连接节点 1 · TUN 网卡就绪", font(21, True), GLOW_B)], 236, card_y + 168, "left"), connected)
        )


@slide(3.8)
def safety(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("安全与自定义", H2_F, TEXT)], W // 2, 120 + appear), alpha))
    frame.alpha_composite(
        fade(text_layer([("本机数据本机保存，公开产物均带代码签名", BODY_F, MUTED)], W // 2, 196 + appear), alpha * 0.95)
    )
    items = [
        ("代码签名", "发布产物 Authenticode 签名 + SHA256SUMS 校验"),
        ("令牌加密", "微软 / LittleSkin 令牌经 DPAPI 加密落盘"),
        ("路径自定义", "数据目录、实例目录、共享游戏目录随意指定"),
        ("镜像可选", "官方 / BMCLAPI / 自动选优，全部可回退"),
    ]
    for index, (title, detail) in enumerate(items):
        x = 150 + (index % 2) * (W - 300) // 2
        y = 268 + (index // 2) * 130 + appear
        card_alpha = ease_out(clamp01((local - index * 0.3) / 0.5))
        if card_alpha <= 0:
            continue
        frame.alpha_composite(fade(panel_layer(x, y, (W - 300) // 2 - 20, 106), card_alpha))
        frame.alpha_composite(
            fade(text_layer([(title, font(24, True), TEXT)], x + 30, y + 16, "left"), card_alpha)
        )
        frame.alpha_composite(
            fade(text_layer([(detail, font(19), MUTED)], x + 30, y + 58, "left"), card_alpha)
        )


@slide(4.0)
def cta(local: float, frame: Image.Image, draw: ImageDraw.ImageDraw):
    appear, alpha = ease_slide(local / 0.7)
    frame.alpha_composite(fade(text_layer([("立即下载 ", TITLE_F, GLOW_A), ("Aurora", TITLE_F, GLOW_B)], W // 2, 190 + appear), alpha))
    pulse = 0.5 + 0.5 * math.sin(local * math.tau * 0.6)
    frame.alpha_composite(
        fade(
            text_layer(
                [("mcdles.us.ci", font(40, True), tuple(int(c * (0.7 + 0.3 * pulse)) for c in GLOW_B))],
                W // 2,
                320 + appear,
            ),
            alpha,
        )
    )
    frame.alpha_composite(
        fade(
            text_layer(
                [("标准版 / 锁区版 · 安装版 / 便携版 · SHA256SUMS 校验", BODY_F, MUTED)],
                W // 2,
                402 + appear,
            ),
            alpha,
        )
    )
    frame.alpha_composite(
        fade(
            text_layer(
                [("开源：github.com/xcqm12/aurora-launcher", SMALL_F, MUTED)],
                W // 2,
                466 + appear,
            ),
            alpha,
        )
    )


def crossfade(a: Image.Image, b: Image.Image, t: float) -> Image.Image:
    return Image.blend(a, b, clamp01(t))


def render_frame(global_index: int, total_frames: int) -> Image.Image:
    # build cumulative timeline with 0.45s crossfades
    fade_frames = int(0.45 * FPS)
    timeline = []
    cursor = 0
    for index, item in enumerate(SLIDES):
        length = int(item["duration"] * FPS)
        timeline.append((index, cursor, cursor + length))
        cursor += length - (fade_frames if index < len(SLIDES) - 1 else 0)
    total = cursor + fade_frames

    g_index = min(global_index, total - 1)
    for slide_index, start, end in timeline:
        if g_index < end:
            local = (g_index - start) / FPS
            frame = background(g_index, total)
            draw = ImageDraw.Draw(frame, "RGBA")
            SLIDES[slide_index]["render"](max(local, 0.0), frame, draw)
            # crossfade into the next slide near its tail
            if slide_index < len(SLIDES) - 1 and g_index >= end - fade_frames:
                t = (g_index - (end - fade_frames)) / fade_frames
                nxt_start = end
                nxt_local = (g_index - nxt_start) / FPS
                nxt = background(g_index, total)
                nxt_draw = ImageDraw.Draw(nxt, "RGBA")
                SLIDES[slide_index + 1]["render"](max(nxt_local, 0.0), nxt, nxt_draw)
                frame = crossfade(frame, nxt, t)
            return frame
    return background(total - 1, total)


def main() -> None:
    output = Path(sys.argv[1] if len(sys.argv) > 1 else "aurora-promo.mp4")
    durations = sum(item["duration"] for item in SLIDES)
    fade_total = 0.45 * (len(SLIDES) - 1)
    total_frames = int((durations - fade_total + 0.45) * FPS)
    print(f"rendering {total_frames} frames ({durations - fade_total:.1f}s) at {W}x{H} @{FPS}fps")

    ffmpeg = None
    try:
        import imageio_ffmpeg

        ffmpeg = imageio_ffmpeg.get_ffmpeg_exe()
    except ImportError:
        ffmpeg = "ffmpeg"

    process = subprocess.Popen(
        [
            ffmpeg,
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            f"{W}x{H}",
            "-r",
            str(FPS),
            "-i",
            "-",
            "-c:v",
            "libx264",
            "-preset",
            "medium",
            "-crf",
            "22",
            "-pix_fmt",
            "yuv420p",
            "-movflags",
            "+faststart",
            str(output),
        ],
        stdin=subprocess.PIPE,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    for index in range(total_frames):
        frame = render_frame(index, total_frames).convert("RGB")
        process.stdin.write(frame.tobytes())
        if index % 60 == 0:
            print(f"  frame {index}/{total_frames}")
    process.stdin.close()
    process.wait()
    if process.returncode != 0:
        raise SystemExit(f"ffmpeg exited with {process.returncode}")
    print(f"written {output}")

    poster_index = 0
    cursor = 0
    for index, item in enumerate(SLIDES):
        cursor += int(item["duration"] * FPS)
    render_frame(total_frames - int(3.6 * FPS), total_frames).convert("RGB").save(
        output.with_suffix(".jpg"), quality=88
    )
    print(f"written {output.with_suffix('.jpg')}")


if __name__ == "__main__":
    main()
