"""ProCraft icon v2: realistic peacock feather, drawn natively in VectorCraft
headless batch. All geometry computed here (seeded organic jitter); no stock
art, no tracing, no copied paths. Eye = gradated spade, barbs = dense curved
wisps, backdrop = vignette teal."""
import json, random, subprocess

CLI = r"C:\Program Files\VectorCraft\vectorcraft-cli.exe"
import os
HERE = os.path.dirname(os.path.abspath(__file__))
TMP = HERE
MASTER = os.path.join(HERE, "icon-1024.png")

rng = random.Random(7)
cmds = []
def cmd(cid, params=None):
    cmds.append("--cmd"); cmds.append(cid)
    if params is not None:
        cmds.append("--params"); cmds.append(json.dumps(params))

def fnum(v):
    s = f"{v:.1f}"
    return s[:-2] if s.endswith(".0") else s

def rounded_rect(x, y, w, h, r):
    return (
        f"M{fnum(x+r)} {fnum(y)}H{fnum(x+w-r)}"
        f"Q{fnum(x+w)} {fnum(y)} {fnum(x+w)} {fnum(y+r)}V{fnum(y+h-r)}"
        f"Q{fnum(x+w)} {fnum(y+h)} {fnum(x+w-r)} {fnum(y+h)}H{fnum(x+r)}"
        f"Q{fnum(x)} {fnum(y+h)} {fnum(x)} {fnum(y+h-r)}V{fnum(y+r)}"
        f"Q{fnum(x)} {fnum(y)} {fnum(x+r)} {fnum(y)}Z"
    )

def ellipse(cx, cy, rx, ry):
    k = 0.5522847498
    return (
        f"M{fnum(cx-rx)} {fnum(cy)}"
        f"C{fnum(cx-rx)} {fnum(cy-k*ry)} {fnum(cx-k*rx)} {fnum(cy-ry)} {fnum(cx)} {fnum(cy-ry)}"
        f"C{fnum(cx+k*rx)} {fnum(cy-ry)} {fnum(cx+rx)} {fnum(cy-k*ry)} {fnum(cx+rx)} {fnum(cy)}"
        f"C{fnum(cx+rx)} {fnum(cy+k*ry)} {fnum(cx+k*rx)} {fnum(cy+ry)} {fnum(cx)} {fnum(cy+ry)}"
        f"C{fnum(cx-k*rx)} {fnum(cy+ry)} {fnum(cx-rx)} {fnum(cy+k*ry)} {fnum(cx-rx)} {fnum(cy)}Z"
    )

# Spade/teardrop eye: full crown, tapered soft point at the base.
EYE = ("M512 145"
       "C600 145 662 210 662 300"
       "C662 400 580 470 512 520"
       "C444 470 362 400 362 300"
       "C362 210 424 145 512 145Z")

def solid(d, fill, opacity=None):
    cmd("path.create", {"d": d})
    cmd("paint.setFill", {"color": fill})
    cmd("paint.setStroke", {"none": True})
    if opacity is not None:
        cmd("object.setProps", {"opacity": opacity})

def grad(d, stops):
    cmd("path.create", {"d": d})
    cmd("paint.setFill", {"gradient": {"kind": "radial", "stops": stops}})
    cmd("paint.setStroke", {"none": True})

def barb(x1, y1, cx, cy, x2, y2, color, weight, opacity=None):
    cmd("path.create", {"d": f"M{fnum(x1)} {fnum(y1)}Q{fnum(cx)} {fnum(cy)} {fnum(x2)} {fnum(y2)}"})
    cmd("paint.setFill", {"none": True})
    cmd("paint.setStroke", {"color": color})
    cmd("stroke.set", {"weight": round(weight, 1), "cap": "round"})
    if opacity is not None:
        cmd("object.setProps", {"opacity": opacity})

CX = 512.0
cmd("file.new", {"width": 1024, "height": 1024, "units": "Pixels",
                 "title": "ProCraft icon v2"})

# 1. Vignette backdrop: lighter teal heart melting into near-black edges.
cmd("path.create", {"d": rounded_rect(0, 0, 1024, 1024, 232)})
cmd("paint.setFill", {"gradient": {"kind": "radial", "stops": [
    {"offset": 0.0, "color": "#145451", "opacity": 1},
    {"offset": 0.55, "color": "#0B3D3A", "opacity": 1},
    {"offset": 1.0, "color": "#041E1D", "opacity": 1},
]}})
cmd("paint.setStroke", {"none": True})

# 2. Breath of warm aura behind the eye (fully fading, no disc edge).
grad(ellipse(CX, 330, 300, 300), [
    {"offset": 0.0, "color": "#D9A441", "opacity": 0.28},
    {"offset": 0.6, "color": "#D9A441", "opacity": 0.10},
    {"offset": 1.0, "color": "#D9A441", "opacity": 0},
])

# 3. Dense curved barb wisps (48 pairs, organic jitter, depth opacities).
palette = [("#2FA36B", 40), ("#1E8C7A", 25), ("#8FC25A", 15),
           ("#D9A441", 12), ("#A87F2A", 8)]
def pick():
    r = rng.uniform(0, 100)
    acc = 0
    for col, wt in palette:
        acc += wt
        if r <= acc:
            return col
    return palette[0][0]

N = 48
for i in range(N):
    t = i / (N - 1)
    yb = 960 - t * 445.0 + rng.uniform(-6, 6)
    L = 292 - t * 152.0 + rng.uniform(-26, 26)
    w = max(1.8, 4.4 - t * 2.0 + rng.uniform(-0.5, 0.5))
    rise = L * rng.uniform(0.38, 0.55)
    bow = rng.uniform(8, 26)          # upward bow of the curve
    droop = rng.uniform(2, 16)        # tip falls back down
    for s in (-1.0, 1.0):
        x1, y1 = CX + s * 8.0, yb
        x2 = CX + s * (10.0 + L)
        y2 = yb - rise + droop
        # keep inside the keyline
        if abs(x2 - CX) > 466:
            x2 = CX + s * 466.0
        y2 = min(max(y2, 60.0), 964.0)
        mx, my = (x1 + x2) / 2, (y1 + y2) / 2 - bow
        op = None
        if rng.random() < 0.25:
            op = rng.randint(50, 75)
        barb(x1, y1, mx, my, x2, y2, pick(), w, op)

# 4. Slim shaft under the barb roots.
solid("M505 992L519 992L514 480L510 480Z", "#E4D3A0")
# 5. Dark rim + gradated spade eye (gold rim melting to midnight heart).
solid("M512 138C605 138 669 208 669 300C669 403 582 476 512 527C442 476 355 403 355 300C355 208 419 138 512 138Z",
      "#0A2A22")
grad(EYE, [
    {"offset": 0.0, "color": "#101038", "opacity": 1},
    {"offset": 0.30, "color": "#1E46B4", "opacity": 1},
    {"offset": 0.50, "color": "#14A08C", "opacity": 1},
    {"offset": 0.68, "color": "#1F9D63", "opacity": 1},
    {"offset": 0.82, "color": "#8A6D1F", "opacity": 1},
    {"offset": 0.93, "color": "#C9A227", "opacity": 1},
    {"offset": 1.0, "color": "#7A5F1C", "opacity": 1},
])
# 6. Shaft notch where the eye wraps the stem.
solid("M496 468L528 468L512 524Z", "#0A2A22")
# 7. Soft gloss.
grad(ellipse(478, 250, 26, 55), [
    {"offset": 0.0, "color": "#FFFFFF", "opacity": 0.9},
    {"offset": 1.0, "color": "#FFFFFF", "opacity": 0},
])
# 8. Keyline frame.
cmd("path.create", {"d": rounded_rect(30, 30, 964, 964, 205)})
cmd("paint.setFill", {"none": True})
cmd("paint.setStroke", {"color": "#D9A441"})
cmd("stroke.set", {"weight": 6})
cmd("object.setProps", {"opacity": 55})

r = subprocess.run([CLI, "run"] + cmds +
                   ["--export", MASTER,
                    "--export", os.path.join(TMP, "icon.svg")],
                   capture_output=True, text=True)
print("RC:", r.returncode)
n_cmd = sum(1 for l in r.stdout.splitlines() if '"step": "cmd"' in l or '"step":"cmd"' in l)
print("commands ok:", n_cmd)
tail = r.stdout.strip().splitlines()[-3:]
for line in tail:
    print(line[-220:])
print("STDERR:", r.stderr[-500:])
