# -*- coding: utf-8 -*-
"""生成 classroom-gui 应用图标（Task 1b）。

教师蓝 #2563EB 圆形 + 白色上下双箭头（共享/收发），透明底。
输出：32x32.png / 128x128.png / 128x128@2x.png(256) / icon.ico
依赖：Pillow（运行于 DSH 内置 Python）。变更图标后重跑本脚本即可。
"""
from pathlib import Path

from PIL import Image, ImageDraw

BLUE = (37, 99, 235, 255)  # #2563EB
WHITE = (255, 255, 255, 255)

HERE = Path(__file__).resolve().parent


def make(size: int) -> Image.Image:
    img = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    d = ImageDraw.Draw(img)
    m = max(1, round(size / 16))
    # 圆底
    d.ellipse([m, m, size - 1 - m, size - 1 - m], fill=BLUE)
    # 白色上下双箭头（竖线 + 两个三角头）
    u = size / 10
    cx = size / 2
    w = max(2, round(u * 0.55))
    top, bot = 2.5 * u + m, size - 2.5 * u - m
    d.line([(cx, bot), (cx, top)], fill=WHITE, width=w)
    ah, aw = 1.1 * u, 1.5 * u
    d.polygon([(cx, top - ah), (cx - aw, top + ah), (cx + aw, top + ah)], fill=WHITE)
    d.polygon([(cx, bot + ah), (cx - aw, bot - ah), (cx + aw, bot - ah)], fill=WHITE)
    return img


def main() -> None:
    make(32).save(HERE / "32x32.png")
    make(128).save(HERE / "128x128.png")
    make(256).save(HERE / "128x128@2x.png")
    make(256).save(
        HERE / "icon.ico",
        sizes=[(16, 16), (24, 24), (32, 32), (48, 48), (64, 64), (128, 128), (256, 256)],
    )
    print("icons written:", sorted(p.name for p in HERE.glob("*")))


if __name__ == "__main__":
    main()
