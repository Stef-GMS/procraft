"""Linear-gradient probe + first layer-stack draft (fast iteration)."""
import json, subprocess

CLI = r"C:\Program Files\VectorCraft\vectorcraft-cli.exe"
import os
HERE = os.path.dirname(os.path.abspath(__file__))
TMP = HERE

cmds = []
def cmd(cid, params=None):
    cmds.append("--cmd"); cmds.append(cid)
    if params is not None:
        cmds.append("--params"); cmds.append(json.dumps(params))

def fnum(v):
    s = f"{v:.1f}"
    return s[:-2] if s.endswith(".0") else s

def rrect(x, y, w, h, r):
    return (
        f"M{fnum(x+r)} {fnum(y)}H{fnum(x+w-r)}"
        f"Q{fnum(x+w)} {fnum(y)} {fnum(x+w)} {fnum(y+r)}V{fnum(y+h-r)}"
        f"Q{fnum(x+w)} {fnum(y+h)} {fnum(x+w-r)} {fnum(y+h)}H{fnum(x+r)}"
        f"Q{fnum(x)} {fnum(y+h)} {fnum(x)} {fnum(y+h-r)}V{fnum(y+r)}"
        f"Q{fnum(x)} {fnum(y)} {fnum(x+r)} {fnum(y)}Z"
    )

cmd("file.new", {"width": 1024, "height": 1024, "units": "Pixels",
                 "title": "ProCraft icon v3 draft"})
# Tile.
cmd("path.create", {"d": rrect(0, 0, 1024, 1024, 232)})
cmd("paint.setFill", {"color": "#0B3D3A"})
cmd("paint.setStroke", {"none": True})
# Back plate (deep emerald, darker for depth).
cmd("path.create", {"d": rrect(165, 373, 440, 440, 102)})
cmd("paint.setFill", {"color": "#14655F"})
cmd("paint.setStroke", {"none": True})
# Middle plate (teal-green).
cmd("path.create", {"d": rrect(292, 292, 440, 440, 102)})
cmd("paint.setFill", {"color": "#2FA36B"})
cmd("paint.setStroke", {"none": True})
# Front plate: gold with diagonal linear gradient (lit finish).
cmd("path.create", {"d": rrect(419, 211, 440, 440, 102)})
cmd("paint.setFill", {"gradient": {"kind": "linear", "angle": 135, "stops": [
    {"offset": 0.0, "color": "#F2E3AC", "opacity": 1},
    {"offset": 0.55, "color": "#C9A227", "opacity": 1},
    {"offset": 1.0, "color": "#8A6D1F", "opacity": 1},
]}})
cmd("paint.setStroke", {"none": True})
# Keyline frame.
cmd("path.create", {"d": rrect(30, 30, 964, 964, 205)})
cmd("paint.setFill", {"none": True})
cmd("paint.setStroke", {"color": "#D9A441"})
cmd("stroke.set", {"weight": 6})
cmd("object.setProps", {"opacity": 55})

r = subprocess.run([CLI, "run"] + cmds +
                   ["--export", TMP + "\\icon-1024.png",
                    "--export", TMP + "\\icon.svg"],
                   capture_output=True, text=True)
print("RC:", r.returncode)
print("STDERR:", r.stderr[-300:])
