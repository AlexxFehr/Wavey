"""Pilot round robin on wave-based room acoustics (Fratoni et al.): the
Bologna lecture hall, source S1, mean over R1–R26. Measurement and inputs
from the dataset; FDTD and GA T30 from Cingolani et al. 2021 (Appl. Sci. 11,
2445, Table 6). Wavey from `rrt_compare`.

python3 docs/validation/plot_rrt.py  ->  docs/validation/rrt-classroom.png
"""
import csv
import os
import matplotlib.pyplot as plt
import numpy as np

here = os.path.dirname(__file__)
rows = {}
for r in csv.reader(open(os.path.join(here, "rrt-classroom.csv"))):
    if r[0] != "tool":
        rows[(r[0], r[1])] = [float(x) for x in r[2:]]
BANDS = ["125", "250", "500", "1k", "2k", "4k"]
PARAMS = [("EDT", "EDT (s)"), ("T30", "T30 (s)"), ("C50", "C50 (dB)"), ("Ts", "Ts (ms)")]
STYLE = {
    "Wavey": dict(color="#ff7f0e", lw=2.4, marker="o", ms=4),
    "FDTD (Cingolani 2021)": dict(color="#1f77b4", lw=1.2, marker="^", ms=5),
    "GA (Cingolani 2021)": dict(color="#2ca02c", lw=1.2, marker="s", ms=4),
}


def err(p, s, m):
    s, m = np.array(s), np.array(m)
    if p in ("EDT", "T30"):
        return np.mean(np.abs(s - m) / (0.05 * m))
    return np.mean(np.abs(s - m) / (1.0 if p == "C50" else 10.0))


fig, axes = plt.subplots(1, 4, figsize=(17, 4.2), dpi=140)
x = np.arange(6)
for ax, (p, label) in zip(axes, PARAMS):
    m = rows[("Measured", p)]
    ax.plot(x, m, color="black", lw=1.6, marker="x", label="Measured", zorder=5)
    if p in ("EDT", "T30"):
        ax.fill_between(x, np.array(m) * 0.95, np.array(m) * 1.05, color="#999", alpha=0.2, lw=0, label="±1 JND")
    for tool, st in STYLE.items():
        if (tool, p) in rows:
            ax.plot(x, rows[(tool, p)], label=f"{tool} ({err(p, rows[(tool, p)], m):.2f} JND)", **st)
    ax.set_xticks(x, BANDS)
    ax.set_xlabel("Octave band (Hz)")
    ax.set_title(label)
    ax.grid(True, color="#e6e6e6", lw=0.6)
    ax.legend(fontsize=7)
fig.suptitle("Pilot round robin, wave-based simulations: lecture hall (≈ 830 m³), S1, mean over R1–R26", fontsize=10)
fig.tight_layout()
out = os.path.join(here, "rrt-classroom.png")
fig.savefig(out)
print(out)
