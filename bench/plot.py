#!/usr/bin/env python3
"""Draws the README charts from bench/results.json into assets/, as SVG in light and dark.

    python3 bench/plot.py

No dependencies. Colors come from a palette checked for colour-blind separation in both modes.
"""
import json, math, os

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
W, H = 720, 380
LEFT, RIGHT, TOP, BOTTOM = 64, 150, 96, 52  # plot margins; the right one holds end labels

THEMES = {
    "light": {"ink": "#0b0b0b", "ink2": "#52514e", "muted": "#898781", "grid": "#e1e0d9",
              "axis": "#c3c2b7", "surface": "#ffffff", "series": ["#2a78d6", "#eb6834", "#1baf7a"],
              "context": "#a3a29b"},
    "dark": {"ink": "#f0efec", "ink2": "#c3c2b7", "muted": "#898781", "grid": "#2c2c2a",
             "axis": "#383835", "surface": "#0d1117", "series": ["#3987e5", "#d95926", "#199e70"],
             "context": "#6b6a65"},
}


def compact(v, unit=""):
    for div, suffix in ((1e6, "M"), (1e3, "K")):
        if v >= div:
            s = f"{v / div:.1f}".rstrip("0").rstrip(".")
            return f"{s}{suffix}{unit}"
    return f"{v:g}{unit}"


def chart(title, subtitle, xs, series, y_ticks, y_fmt, log_y, theme):
    """series: [(label, values, color_key, end_label)]; color_key is a slot index or 'context'."""
    t = THEMES[theme]
    pw, ph = W - LEFT - RIGHT, H - TOP - BOTTOM
    lx = [math.log10(x) for x in xs]
    x_of = lambda x: LEFT + (math.log10(x) - lx[0]) / (lx[-1] - lx[0]) * pw
    y0, y1 = y_ticks[0], y_ticks[-1]
    if log_y:
        y_of = lambda v: TOP + ph - (math.log10(v) - math.log10(y0)) / (math.log10(y1) - math.log10(y0)) * ph
    else:
        y_of = lambda v: TOP + ph - (v - y0) / (y1 - y0) * ph
    color = lambda k: t["context"] if k == "context" else t["series"][k]

    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" viewBox="0 0 {W} {H}" '
           f'font-family="system-ui, -apple-system, \'Segoe UI\', sans-serif" role="img" aria-label="{title}">',
           f'<title>{title}</title>',
           f'<text x="{LEFT - 40}" y="30" font-size="17" font-weight="600" fill="{t["ink"]}">{title}</text>',
           f'<text x="{LEFT - 40}" y="52" font-size="13" fill="{t["ink2"]}">{subtitle}</text>']
    # Legend: a short line key beside each name, in series order.
    lx_pos = LEFT - 40
    for label, _, key, _ in series:
        out.append(f'<line x1="{lx_pos}" y1="74" x2="{lx_pos + 18}" y2="74" stroke="{color(key)}" stroke-width="2.5" stroke-linecap="round"/>')
        out.append(f'<text x="{lx_pos + 24}" y="78" font-size="12" fill="{t["ink2"]}">{label}</text>')
        lx_pos += 24 + 6.0 * len(label) + 26
    # Grid and y ticks.
    for v in y_ticks:
        y = y_of(v)
        stroke = t["axis"] if v == y0 else t["grid"]
        out.append(f'<line x1="{LEFT}" y1="{y:.1f}" x2="{LEFT + pw}" y2="{y:.1f}" stroke="{stroke}" stroke-width="1"/>')
        out.append(f'<text x="{LEFT - 10}" y="{y + 4:.1f}" font-size="12" fill="{t["muted"]}" text-anchor="end" '
                   f'style="font-variant-numeric: tabular-nums">{y_fmt(v)}</text>')
    for x in xs:
        out.append(f'<text x="{x_of(x):.1f}" y="{TOP + ph + 22}" font-size="12" fill="{t["muted"]}" text-anchor="middle" '
                   f'style="font-variant-numeric: tabular-nums">{x:,}</text>')
    out.append(f'<text x="{LEFT + pw / 2:.1f}" y="{H - 8}" font-size="12" fill="{t["muted"]}" text-anchor="middle">notes in the notebook</text>')
    # Lines, with ringed markers and a direct label at the end.
    for label, values, key, end in series:
        pts = [(x_of(x), y_of(v)) for x, v in zip(xs, values)]
        path = " ".join(f"{'M' if i == 0 else 'L'}{px:.1f},{py:.1f}" for i, (px, py) in enumerate(pts))
        out.append(f'<path d="{path}" fill="none" stroke="{color(key)}" stroke-width="2" stroke-linejoin="round" stroke-linecap="round"/>')
        for px, py in pts:
            out.append(f'<circle cx="{px:.1f}" cy="{py:.1f}" r="4" fill="{color(key)}" stroke="{t["surface"]}" stroke-width="2"/>')
        ex, ey = pts[-1]
        out.append(f'<text x="{ex + 12:.1f}" y="{ey + 4:.1f}" font-size="12" fill="{t["ink"]}" font-weight="600">{end}</text>')
    out.append("</svg>")
    return "\n".join(out) + "\n"


def main():
    r = json.load(open(os.path.join(ROOT, "bench", "results.json")))
    sizes = r["sizes"]
    xs = [s["notes"] for s in sizes]
    tok = lambda b: b / 4
    md = [tok(s["lookup"]["markdown_bytes"]) for s in sizes]
    nbt = [tok(s["lookup"]["nb_bytes"]) for s in sizes]
    cmd = lambda name: [s["commands"][name]["ms"] for s in sizes]
    file_mb = [s["bytes"] / 1e6 for s in sizes]
    rss = [max(c["rss_mb"] for c in s["commands"].values()) for s in sizes]
    os.makedirs(os.path.join(ROOT, "assets"), exist_ok=True)

    charts = {
        "bench-lookup": lambda th: chart(
            "Tokens read to answer one question",
            "“What do my notes say about this topic?” (tokens ≈ bytes ÷ 4, log scale)",
            xs, [("Reading NOTES.md", md, "context", f"NOTES.md {compact(md[-1])}"),
                 ("nb ids + list -i + show -n", nbt, 0, f"nb {compact(nbt[-1])}")],
            [1e3, 1e4, 1e5, 1e6, 1e7], compact, True, th),
        "bench-latency": lambda th: chart(
            "Command time by notebook size",
            "Median wall time, process start included",
            xs, [("nb ids", cmd("ids"), 0, f"ids {cmd('ids')[-1]:.0f} ms"),
                 ("nb list -i", cmd("list -i"), 1, f"list -i {cmd('list -i')[-1]:.0f} ms"),
                 ("nb edit", cmd("edit"), 2, f"edit {cmd('edit')[-1]:.0f} ms")],
            [0, 10, 20, 30, 40], lambda v: f"{v:g} ms", False, th),
        "bench-memory": lambda th: chart(
            "Peak memory by notebook size",
            "nb streams one note at a time, so memory stays flat",
            xs, [("Notebook file size", file_mb, "context", f"file {file_mb[-1]:.0f} MB"),
                 ("nb peak memory", rss, 0, f"nb {rss[-1]:.1f} MB")],
            [0, 10, 20, 30, 40, 50], lambda v: f"{v:g} MB", False, th),
    }
    for name, draw in charts.items():
        for th in THEMES:
            path = os.path.join(ROOT, "assets", f"{name}-{th}.svg")
            with open(path, "w") as f:
                f.write(draw(th))
            print(f"wrote {os.path.relpath(path, ROOT)}")


if __name__ == "__main__":
    main()
