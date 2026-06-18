#!/usr/bin/env python3
"""Generate tarn's TUI-style banner + mascot as SVGs built from terminal cells.

No smooth gradients: the tarnish is quantized into discrete copper->patina
color steps, the wordmark is a pixel font, and the frame mimics a real TUI
(box-drawing border + inverse-video status bar).
"""

# --- palette: polished copper -> tarnish -> patina, as discrete cells ---------
TARNISH = ["#f2c089", "#dd9648", "#c7752e", "#a05a1f", "#6f4524", "#46594a", "#2f5d4f"]
INK   = "#0a0f14"
DIMC  = "#7a4a22"   # dim copper (border)
MINT  = "#7fd1b0"   # mascot eyes / accents
TEXT  = "#9fb0c0"   # muted foreground

def step(p):
    """Map 0..1 to a discrete tarnish color."""
    i = int(p * len(TARNISH))
    return TARNISH[min(i, len(TARNISH) - 1)]

# --- 5x7 pixel font (lowercase) ----------------------------------------------
GLYPHS = {
 't': ["..#..", "..#..", ".###.", "..#..", "..#..", "..#..", "..##."],
 'a': [".....", ".....", ".###.", "#...#", ".####", "#...#", ".####"],
 'r': [".....", ".....", "#.##.", "##...", "#....", "#....", "#...."],
 'n': [".....", ".....", "####.", "#..#.", "#..#.", "#..#.", "#..#."],
}

def esc(s):
    return s.replace("&", "&amp;").replace("<", "&lt;")

def rect(x, y, w, h, fill, extra=""):
    return f'<rect x="{x}" y="{y}" width="{w}" height="{h}" fill="{fill}"{(" "+extra) if extra else ""}/>'

# ============================ BANNER =========================================
def banner():
    W, H = 1200, 320
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
           f'viewBox="0 0 {W} {H}" role="img" aria-label="tarn — a tiny, understandable terminal editor">']
    # solid terminal background
    out.append(rect(0, 0, W, H, INK))

    # faint character-cell grid (texture, not noise)
    grid = ['<g stroke="#ffffff" stroke-opacity="0.022" stroke-width="1">']
    for gx in range(0, W, 24):
        grid.append(f'<line x1="{gx}" y1="0" x2="{gx}" y2="{H}"/>')
    for gy in range(0, H, 24):
        grid.append(f'<line x1="0" y1="{gy}" x2="{W}" y2="{gy}"/>')
    grid.append('</g>')
    out.append("".join(grid))

    # TUI box-drawing border (crisp, square corners) + corner glyphs
    bx, by, bw, bh = 12, 12, W - 24, H - 24
    out.append(f'<rect x="{bx}" y="{by}" width="{bw}" height="{bh}" fill="none" '
               f'stroke="{DIMC}" stroke-width="2"/>')
    mono = "'SF Mono','JetBrains Mono',Menlo,Consolas,monospace"
    # title sits on the top border, breaking the line like ┤ tarn ├
    title = " tarn — a tiny terminal editor "
    out.append(rect(150, 4, 360, 18, INK))  # gap so the line appears broken
    out.append(f'<text x="172" y="19" font-family="{mono}" font-size="15" '
               f'letter-spacing="2" fill="{DIMC}">{esc(title.strip())}</text>')

    # --- wordmark "tarn" as discrete pixel cells --------------------------
    word = "tarn"
    C = 22                 # cell size
    GAP = 1                # empty columns between glyphs
    X0, Y0 = 70, 72
    # total grid columns for quantizing color left->right
    total_cols = sum(5 for _ in word) + GAP * (len(word) - 1)
    col_cursor = 0
    cells = []
    for ch in word:
        g = GLYPHS[ch]
        for r in range(7):
            for c in range(5):
                if g[r][c] == '#':
                    wc = col_cursor + c
                    color = step(wc / (total_cols - 1))
                    x = X0 + wc * C
                    y = Y0 + r * C
                    # tiny inset so cells read as separate blocks, like a grid
                    cells.append(rect(x, y, C - 2, C - 2, color))
        col_cursor += 5 + GAP
    out.append("".join(cells))

    # block cursor cell, one column after the word, full glyph height
    cur_x = X0 + total_cols * C + C
    out.append(rect(cur_x, Y0, C - 2, 7 * C - 2, "#2f5d4f"))

    # --- inverse-video status bar (like the real editor) ------------------
    sb_y = H - 50
    sb_h = 26
    out.append(rect(bx + 2, sb_y, bw - 4, sb_h, "#c7752e"))      # copper bar
    out.append(f'<text x="28" y="{sb_y + 18}" font-family="{mono}" font-size="15" '
               f'letter-spacing="1" font-weight="700" fill="{INK}">'
               f'tarn*  $ a tiny, understandable terminal editor</text>')
    out.append(f'<text x="{W - 28}" y="{sb_y + 18}" text-anchor="end" '
               f'font-family="{mono}" font-size="15" letter-spacing="1" '
               f'font-weight="700" fill="{INK}">^S save   ^Q quit   Ln 1, Col 1</text>')

    # --- mascot, pixel version (sharp cells) ------------------------------
    out.append(mascot_group(ox=1006, oy=70, cell=13))

    out.append('</svg>')
    return "\n".join(out)

# ============================ MASCOT =========================================
# Cu: an 11-wide x 12-tall pixel critter — a terminal-cursor robot whose copper
# body tarnishes toward patina at the lower-right corner. Legend:
#   B body (copper, tarnishing by position)  E eye(mint)  P pupil(ink)
#   M mouth(ink)  A arm(copper)  F foot(copper)  T antenna stalk  C antenna tip
MASCOT = [
 ".....C.....",
 ".....T.....",
 "..BBBBBBB..",
 ".BBBBBBBBB.",
 "ABBEEBEEBBA",
 "ABBEPBEPBBA",
 ".BBBBBBBBB.",
 ".BBBMMMBBB.",
 ".BBBBBBBBB.",
 "..BBBBBBB..",
 "..FF...FF..",
 "...........",
]

# Copper body steps, only the last reaching patina — keeps Cu clearly a copper
# creature that's tarnishing at one corner.
BODY = ["#e0a35a", "#c7752e", "#a05a1f", "#6f4524", "#2f5d4f"]

def mascot_group(ox, oy, cell):
    rows = len(MASCOT)
    cols = len(MASCOT[0])
    diag_max = (cols - 1) + (rows - 1)
    out = ['<g>']
    for r in range(rows):
        for c in range(cols):
            k = MASCOT[r][c]
            if k == '.':
                continue
            x = ox + c * cell
            y = oy + r * cell
            if k == 'B':
                i = int((c + r) / diag_max * len(BODY))
                color = BODY[min(i, len(BODY) - 1)]
            elif k == 'E':
                color = MINT
            elif k == 'P':
                color = INK
            elif k == 'M':
                color = "#3a2a1c"
            elif k == 'C':
                color = MINT
            else:  # A, F, T -> copper
                color = "#c7752e"
            out.append(rect(x, y, cell - 2, cell - 2, color))
    out.append('</g>')
    return "".join(out)

def mascot_svg():
    cols = len(MASCOT[0]); rows = len(MASCOT)
    cell = 18
    pad = 18
    W = cols * cell + pad * 2
    H = rows * cell + pad * 2
    out = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{W}" height="{H}" '
           f'viewBox="0 0 {W} {H}" role="img" aria-label="Cu, the tarn mascot — a terminal-cursor critter">']
    out.append(rect(0, 0, W, H, INK))
    out.append(mascot_group(ox=pad, oy=pad, cell=cell))
    out.append('</svg>')
    return "\n".join(out)

if __name__ == "__main__":
    import os
    here = os.path.dirname(os.path.abspath(__file__))
    with open(os.path.join(here, "banner.svg"), "w") as f:
        f.write(banner() + "\n")
    with open(os.path.join(here, "mascot.svg"), "w") as f:
        f.write(mascot_svg() + "\n")
    print("wrote banner.svg + mascot.svg")
