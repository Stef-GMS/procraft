"""Original peacock-feather app icon for ProCraft, drawn natively in
VectorCraft via headless batch commands. All geometry is computed here;
no stock art, no tracing, no copied paths."""
import json, subprocess, sys

CLI = r"C:\Program Files\VectorCraft\vectorcraft-cli.exe"
import os
HERE = os.path.dirname(os.path.abspath(__file__))
TMP = os.path.join(HERE, "out")
os.makedirs(TMP, exist_ok=True)
TMP = TMP.replace("/", "\\")
MASTER = TMP + r"\icon-1024.png"

cmds = []
def cmd(cid, params=None):
    cmds.append("--cmd"); cmds.append(cid)
    if params is not None:
        cmds.append("--params"); cmds.append(json.dumps(params))

def fnum(v):
    s = f"{v:.1f}"
    return s[:-2] if s.endswith(".0") else s

def ellipse(cx, cy, rx, ry):
    k = 0.5522847498
    return (
        f"M{fnum(cx-rx)} {fnum(cy)}"
        f"C{fnum(cx-rx)} {fnum(cy-k*ry)} {fnum(cx-k*rx)} {fnum(cy-ry)} {fnum(cx)} {fnum(cy-ry)}"
        f"C{fnum(cx+k*rx)} {fnum(cy-ry)} {fnum(cx+rx)} {fnum(cy-k*ry)} {fnum(cx+rx)} {fnum(cy)}"
        f"C{fnum(cx+rx)} {fnum(cy+k*ry)} {fnum(cx+k*rx)} {fnum(cy+ry)} {fnum(cx)} {fnum(cy+ry)}"
        f"C{fnum(cx-k*rx)} {fnum(cy+ry)} {fnum(cx-rx)} {fnum(cy+k*ry)} {fnum(cx-rx)} {fnum(cy)}Z"
    )

def rounded_rect(x, y, w, h, r):
    return (
        f"M{fnum(x+r)} {fnum(y)}H{fnum(x+w-r)}"
        f"Q{fnum(x+w)} {fnum(y)} {fnum(x+w)} {fnum(y+r)}V{fnum(y+h-r)}"
        f"Q{fnum(x+w)} {fnum(y+h)} {fnum(x+w-r)} {fnum(y+h)}H{fnum(x+r)}"
        f"Q{fnum(x)} {fnum(y+h)} {fnum(x)} {fnum(y+h-r)}V{fnum(y+r)}"
        f"Q{fnum(x)} {fnum(y)} {fnum(x+r)} {fnum(y)}Z"
    )

def shape(d, fill, stroke_none=True, opacity=None):
    cmd("path.create", {"d": d})
    cmd("paint.setFill", {"color": fill} if fill else {"none": True})
    if stroke_none:
        cmd("paint.setStroke", {"none": True})
    if opacity is not None:
        cmd("object.setProps", {"opacity": opacity})

def barb(x1, y1, x2, y2, color, weight):
    cmd("path.create", {"d": f"M{fnum(x1)} {fnum(y1)}L{fnum(x2)} {fnum(y2)}"})
    cmd("paint.setFill", {"none": True})
    cmd("paint.setStroke", {"color": color})
    cmd("stroke.set", {"weight": weight, "cap": "round"})

CX = 512.0
cmd("file.new", {"width": 1024, "height": 1024, "units": "Pixels",
                 "title": "ProCraft icon"})

# 1. Deep-teal rounded-square backdrop.
shape(rounded_rect(0, 0, 1024, 1024, 232), "#0B3D3A")
# 2. Soft gold aura behind the eye.
shape(ellipse(CX, 330, 265, 265), "#D9A441", opacity=12)
# 3. Barb fan (13 pairs, shrinking upward, alternating greens + gold accents).
greens = ["#2FA36B", "#1E8C7A", "#8FC25A"]
for i in range(13):
    t = i / 12.0
    yb = 955 - t * 435.0
    L = 300 - t * 165.0
    w = round(10 - t * 4.0, 1)
    col = "#D9A441" if i % 4 == 3 else greens[i % 3]
    for s in (-1.0, 1.0):
        x1, y1 = CX + s * 13.0, yb
        x2, y2 = CX + s * (13.0 + L), yb - L * 0.55
        barb(x1, y1, x2, y2, col, w)
# 4. Cream shaft over the barb roots (ends at the keyline).
shape(f"M499 992L525 992L516 470L508 470Z", "#E9D9A6")
# 5. The eye: concentric original rings (gold, emerald, turquoise, royal, midnight).
for (rx, ry, col) in [(150, 185, "#C9A227"), (116, 148, "#1F9D63"),
                      (82, 110, "#17B89F"), (50, 72, "#1E46B4"),
                      (24, 38, "#191944")]:
    shape(ellipse(CX, 320, rx, ry), col)
# 6. Gloss highlight.
shape(ellipse(494, 282, 10, 20), "#FFFFFF", opacity=85)
# 7. Gold keyline frame.
cmd("path.create", {"d": rounded_rect(30, 30, 964, 964, 205)})
cmd("paint.setFill", {"none": True})
cmd("paint.setStroke", {"color": "#D9A441"})
cmd("stroke.set", {"weight": 7})
cmd("object.setProps", {"opacity": 65})

r = subprocess.run([CLI, "run"] + cmds +
                   ["--export", MASTER,
                    "--export", TMP + r"\icon.svg"],
                   capture_output=True, text=True)
print("RC:", r.returncode)
for line in r.stdout.strip().splitlines():
    try:
        o = json.loads(line)
        print(o.get("command", o.get("step")), "->",
              str(o.get("result", o))[:160])
    except ValueError:
        print(line[-300:])
print("STDERR:", r.stderr[-1000:])
