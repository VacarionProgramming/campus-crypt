#!/usr/bin/env python3
"""Draws hardware/wiring_diagram.png for the output-only Mega 2560 build
(needs matplotlib:  pip install matplotlib)."""
import os
import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt
from matplotlib.patches import FancyBboxPatch

OUT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "wiring_diagram.png"))

fig, ax = plt.subplots(figsize=(26, 19), dpi=100)
ax.set_xlim(-2, 252)
ax.set_ylim(-22, 186)
ax.axis("off")

C = {"max": "#d62728", "lcd": "#8c564b", "i2c": "#17becf", "seg": "#1f77b4", "digit": "#9467bd",
     "pwr": "#c00000", "gnd": "#000000", "res": "#333333"}


def box(x, y, w, h, title, fc):
    ax.add_patch(FancyBboxPatch((x, y), w, h, boxstyle="round,pad=0.5", fc=fc, ec="#333", lw=1.6, zorder=1))
    ax.text(x + w / 2, y + h + 1.5, title, ha="center", va="bottom", fontsize=12.5, weight="bold")


def pin(x, y, label, inside="right", fs=9.5):
    ax.plot([x], [y], "o", ms=5, color="#222", zorder=5)
    if inside == "right":
        ax.text(x + 1.0, y, label, ha="left", va="center", fontsize=fs, family="monospace", zorder=6)
    else:
        ax.text(x - 1.0, y, label, ha="right", va="center", fontsize=fs, family="monospace", zorder=6)
    return (x, y)


def line(xs, ys, color, lw=2.2, ls="-"):
    ax.plot(xs, ys, color=color, lw=lw, ls=ls, solid_joinstyle="round", zorder=3)


def route(a, b, lane, color):
    line([a[0], lane, lane, b[0]], [a[1], a[1], b[1], b[1]], color)


def resistor(x0, x1, y, label):
    zx = [x0 + j * (x1 - x0) / 10 for j in range(11)]
    zy = [y + (0.8 if j % 2 else -0.8) * (0 < j < 10) for j in range(11)]
    line(zx, zy, C["res"], 1.6)
    ax.text((x0 + x1) / 2, y + 1.3, label, ha="center", fontsize=7.5, color="#333")


def note(x, y, text, color, fs=8.5, weight="normal"):
    ax.text(x, y, text, ha="left", va="center", fontsize=fs, color=color, weight=weight, zorder=6)


# ---------------------------------------------------------------- Arduino Mega 2560
names = ["D2", "D3", "D4", "D7", "D8", "D9", "D10", "D11", "D12", "D20 (SDA)", "D21 (SCL)",
         "D22", "D23", "D24", "D25", "D26", "D27", "D28", "D29", "D30", "D31", "D32", "D33", "5V", "GND"]
box(2, 6, 24, 162, "Arduino Mega 2560", "#e8f1fb")
U = {n.split()[0]: pin(26, 164 - i * 6.4, n, inside="left") for i, n in enumerate(names)}
ax.text(14, 1, "USB cable to the Mac (power + serial)\nD0/D1 = serial, leave free.  No joystick, no inputs.",
        ha="left", va="top", fontsize=9.5, style="italic")

# ---------------------------------------------------------------- MAX7219 (top)
box(92, 146, 30, 24, "MAX7219 8x8 matrix (IN side)", "#fff5e6")
M = {k: pin(92, 167 - i * 4.6, k) for i, k in enumerate(["VCC", "GND", "DIN", "CS", "CLK"])}
note(102, M["VCC"][1], "-> + rail", C["pwr"], 8.5, "bold")
note(102, M["GND"][1], "-> - rail", C["gnd"], 8.5, "bold")
route(U["D2"], M["DIN"], 60, C["max"])
route(U["D3"], M["CLK"], 63, C["max"])
route(U["D4"], M["CS"], 66, C["max"])
note(128, 158, "5 female-to-male Duponts", "#555", 9, "bold")

# ---------------------------------------------------------------- DS1307 (top right)
box(168, 146, 26, 22, "DS1307 RTC", "#e9f8fa")
R = {k: pin(168, 165 - i * 5, k) for i, k in enumerate(["GND", "VCC", "SDA", "SCL"])}
note(176, R["GND"][1], "-> - rail", C["gnd"], 8.5, "bold")
note(176, R["VCC"][1], "-> + rail", C["pwr"], 8.5, "bold")
# I2C goes over the top of the MAX7219 and comes down on the RTC's left side
for i, (u, k) in enumerate([("D20", "SDA"), ("D21", "SCL")]):
    lane, top, down = 40 + i * 3, 179 - i * 2.4, 160 - i * 3
    line([U[u][0], lane, lane, down, down, R[k][0]], [U[u][1], U[u][1], top, top, R[k][1], R[k][1]], C["i2c"])
note(200, 156, "4 female-to-male Duponts\nMega I2C = D20/D21\n(A4/A5 do NOT work on a Mega)", "#555", 9, "bold")

# ---------------------------------------------------------------- LCD1602 (middle)
box(92, 97, 34, 41, "LCD1602 (16 pins, on the breadboard)", "#f2eefb")
lcd = [("1 VSS", "-> - rail", C["gnd"]), ("2 VDD", "-> + rail", C["pwr"]), ("3 V0", "-> pot middle", "#555"),
       ("4 RS", None, None), ("5 RW", "-> - rail", C["gnd"]), ("6 E", None, None), ("7-10 D0-D3", "(unused)", "#777"),
       ("11 D4", None, None), ("12 D5", None, None), ("13 D6", None, None), ("14 D7", None, None),
       ("15 A", "-> 220Ω -> + rail", C["pwr"]), ("16 K", "-> - rail", C["gnd"])]
L = {}
for i, (k, t, c) in enumerate(lcd):
    L[k] = pin(92, 136 - i * 3.1, k, fs=8.5)
    if t:
        note(108, L[k][1], t, c, 8.5, "bold")
for i, (u, k) in enumerate([("D7", "4 RS"), ("D8", "6 E"), ("D9", "11 D4"), ("D10", "12 D5"), ("D11", "13 D6"), ("D12", "14 D7")]):
    route(U[u], L[k], 69 + i * 2.6, C["lcd"])
box(140, 122, 24, 15, "10k potentiometer", "#f6f6f6")
ax.text(152, 129.5, "outer leg  -> + rail\nmiddle leg -> LCD pin 3\nouter leg  -> - rail", ha="center", va="center", fontsize=9)
line([126.5, 140], [L["3 V0"][1], 127], "#555", 1.4, "--")
note(140, 112, "13 male-to-male jumpers\n(6 signals + 7 power/contrast)", "#555", 9, "bold")

# ---------------------------------------------------------------- 7-segment (bottom right)
box(176, 19, 44, 78, "4-digit 7-segment 5461AS\n(common cathode, 12 pins)", "#fbeef5")
segs = [("A", 11), ("B", 7), ("C", 4), ("D", 2), ("E", 1), ("F", 10), ("G", 5), ("DP", 3)]
SEG = {}
for i, (s, p) in enumerate(segs):
    u = f"D{22 + i}"
    y = U[u][1]
    SEG[s] = pin(176, y, f"pin {p:>2} = {s}")
    x0, x1 = 146, 160
    line([U[u][0], x0], [y, y], C["seg"])
    resistor(x0, x1, y, "1kΩ")
    line([x1, 176], [y, y], C["seg"])
for i, (d, p) in enumerate([(1, 12), (2, 9), (3, 8), (4, 6)]):
    u = f"D{30 + i}"
    y = U[u][1]
    pin(176, y, f"Digit {d} = pin {p}")
    line([U[u][0], 176], [y, y], C["digit"])
note(176, 12, "12 male-to-male jumpers + 8 x 1kΩ (segments only;\nno resistor on the digit lines)", "#555", 9, "bold")

# ---------------------------------------------------------------- power
line([26, 30], [U["5V"][1]] * 2, C["pwr"], 3)
note(30.5, U["5V"][1], "-> + rail", C["pwr"], 9, "bold")
line([26, 30], [U["GND"][1]] * 2, C["gnd"], 3)
note(30.5, U["GND"][1], "-> - rail (common ground)", C["gnd"], 9, "bold")

# ---------------------------------------------------------------- legend
ax.text(228, 140, "Wire colours", fontsize=11, weight="bold")
legend = [("MAX7219 (D2-D4)", "max"), ("LCD (D7-D12)", "lcd"), ("I2C clock (D20/21)", "i2c"),
          ("7-seg segments (D22-29)", "seg"), ("7-seg digits (D30-33)", "digit"), ("+ rail (5V)", "pwr"), ("- rail (GND)", "gnd")]
for i, (t, k) in enumerate(legend):
    y = 135 - i * 4.0
    line([228, 233], [y, y], C[k], 3.5)
    note(234, y, t, "#000", 9)

notes = (
    "OUTPUT ONLY: LCD1602, 4-digit 7-segment, MAX7219 matrix and DS1307 clock. No joystick or buttons; the keyboard is the only control, and the sketch never reads an input pin.\n"
    "POWER: Mega 5V -> + rail and Mega GND -> - rail (USB powers everything). Put every module on the SAME rail pair. With the optional power module: GND only, never both 5V supplies.\n"
    "Nothing is shared: every wire goes to one place.   WIRES: 9 female-to-male Duponts (MAX7219 5, RTC 4) + 27 male-to-male jumpers (rails 2, LCD 13, 7-seg 12) = 36."
)
ax.text(2, -15, notes, fontsize=10.5, va="center", bbox=dict(boxstyle="round,pad=0.7", fc="#fffbe6", ec="#d0b000"))
ax.text(125, 184, "Campus Crypt status display - wiring (ELEGOO Mega 2560 kit, output only)", ha="center", fontsize=20, weight="bold")

plt.savefig(OUT, bbox_inches="tight", facecolor="white")
print("wrote", OUT)
