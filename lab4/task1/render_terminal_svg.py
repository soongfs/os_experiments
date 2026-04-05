#!/usr/bin/env python3

import html
import pathlib
import re
import sys


ANSI_ESCAPE = re.compile(r"\x1B\[[0-?]*[ -/]*[@-~]")


def sanitize(text: str) -> list[str]:
    text = text.replace("\r\n", "\n").replace("\r", "\n")
    text = ANSI_ESCAPE.sub("", text)
    lines = text.split("\n")
    if lines and lines[-1] == "":
        lines.pop()
    return lines or [""]


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: render_terminal_svg.py <input_txt> <output_svg>", file=sys.stderr)
        return 1

    input_path = pathlib.Path(sys.argv[1])
    output_path = pathlib.Path(sys.argv[2])
    lines = sanitize(input_path.read_text(encoding="utf-8", errors="replace"))

    char_width = 10
    line_height = 24
    padding_x = 24
    padding_y = 24
    title_bar_height = 36
    max_columns = max(len(line) for line in lines)
    width = max(720, padding_x * 2 + max_columns * char_width)
    height = title_bar_height + padding_y * 2 + len(lines) * line_height

    svg_lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
        "  <style>",
        "    .title { font: 14px 'DejaVu Sans Mono', monospace; fill: #d8dee9; }",
        "    .body { font: 18px 'DejaVu Sans Mono', monospace; fill: #e5e9f0; }",
        "  </style>",
        f'  <rect width="{width}" height="{height}" rx="14" fill="#0f172a"/>',
        f'  <rect width="{width}" height="{title_bar_height}" rx="14" fill="#1e293b"/>',
        '  <circle cx="22" cy="18" r="6" fill="#ef4444"/>',
        '  <circle cx="42" cy="18" r="6" fill="#f59e0b"/>',
        '  <circle cx="62" cy="18" r="6" fill="#22c55e"/>',
        f'  <text x="{padding_x + 56}" y="23" class="title">{html.escape(input_path.name)}</text>',
    ]

    for index, line in enumerate(lines, start=1):
        y = title_bar_height + padding_y + index * line_height - 6
        svg_lines.append(
            f'  <text x="{padding_x}" y="{y}" class="body">{html.escape(line)}</text>'
        )

    svg_lines.append("</svg>")
    output_path.write_text("\n".join(svg_lines) + "\n", encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
