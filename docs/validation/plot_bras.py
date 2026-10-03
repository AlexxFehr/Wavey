"""Plots in the style of Dijkman et al. (Forum Acusticum 2025), Figs. 2-4:
mean T20, EDT and D50 per octave band (all source-receiver pairs), measured
with its JND band, the reference simulators, and Wavey.

    python3 docs/validation/plot_bras.py docs/validation/bras-cr2.csv CR2
"""
import math
import csv
import sys
from collections import defaultdict

import matplotlib
matplotlib.use("Agg")
import matplotlib.pyplot as plt

path, room = sys.argv[1], sys.argv[2]
# Optional: output name and a note for the title (e.g. an input variant).
stem = sys.argv[3] if len(sys.argv) > 3 else room.lower()
note = sys.argv[4] if len(sys.argv) > 4 else ""
bands = [63, 125, 250, 500, 1000, 2000, 4000, 8000]
data = defaultdict(dict)
with open(path) as f:
    for row in csv.DictReader(f):
        data[row["param"]][row["tool"]] = [float(row[str(b)]) for b in bands]

style = {
    "PyRoomAcoustics": dict(color="#e8336f", label="PyRoomAcoustics"),
    "I-Simpa": dict(color="#33a1e8", label="I-Simpa"),
    "dg-acoustics": dict(color="#c77dd1", label="dg-acoustics"),
    "Image2Reverb": dict(color="#f0c040", label="Image2Reverb"),
    "Treble": dict(color="#40b080", label="Treble"),
    "Wavey": dict(color="#ff7a00", label="Wavey", lw=2.6, zorder=6),
}
specs = {
    "T20": ("Mean reverberation time (T20) - Measured vs simulated", "Reverberation time T20 [s]", (0, 3), lambda m: (m * 0.95, m * 1.05)),
    "EDT": ("Mean early decay time (EDT) - Measured vs simulated", "Early decay time [s]", (0, 3), lambda m: (m * 0.95, m * 1.05)),
    "D50": ("Mean speech definition index (D50) - Measured vs simulated", "Speech definition [-]", (0, 1), lambda m: (m - 0.05, m + 0.05)),
    "C80": ("Mean clarity (C80) - Measured vs simulated", "Clarity C80 [dB]", (-6, 10), lambda m: (m - 1, m + 1)),
}


def jnd_err(p, sim, meas):
    import math
    e = []
    for s, m in zip(sim, meas):
        if math.isnan(s) or math.isnan(m):
            continue
        e.append(abs(s - m) / (0.05 * m) if p in ("T20", "EDT") else abs(s - m) / (0.05 if p == "D50" else 1.0))
    return sum(e) / len(e) if e else float("nan")


for p, (title, ylabel, ylim, jnd) in specs.items():
    fig, ax = plt.subplots(figsize=(6.4, 4.6), dpi=160)
    meas = data[p]["Measured"]
    lo, hi = zip(*[jnd(m) for m in meas])
    ax.fill_between(bands, lo, hi, color="#cccccc", label="JND", zorder=1)
    ax.plot(bands, meas, color="black", lw=2.2, label="Measured", zorder=5)
    for tool in ["PyRoomAcoustics", "I-Simpa", "dg-acoustics", "Image2Reverb", "Treble", "Wavey"]:
        if tool in data[p]:
            st = dict(style[tool])
            err = jnd_err(p, data[p][tool], meas)
            st["label"] = f"{st['label']} ({err:.2f} JND)"
            ax.plot(bands, data[p][tool], lw=st.pop("lw", 1.3), **st)
    ax.set_xscale("log")
    ax.set_xticks(bands)
    ax.set_xticklabels([str(b) for b in bands])
    ax.minorticks_off()
    ax.set_xlim(63, 8000)
    # The paper's axis, widened when a curve leaves it (CR4 T20/EDT reach ~3.6 s).
    vals = [v for t in data[p].values() for v in t if not math.isnan(v)]
    top = ylim[1] if p in ("D50",) else max(ylim[1], math.ceil(max(vals) * 2) / 2)
    ax.set_ylim(ylim[0] if p != "C80" else min(ylim[0], math.floor(min(vals))), top)
    ax.set_xlabel("Frequency [Hz]")
    ax.set_ylabel(ylabel)
    ax.set_title(f"{title} | BRAS {room}{note}", fontsize=9)
    ax.grid(True, color="#e6e6e6", lw=0.6)
    ax.legend(fontsize=7, loc="lower center", bbox_to_anchor=(0.5, -0.42), ncol=3, frameon=True)
    fig.tight_layout()
    out = f"docs/validation/bras-{stem}-{p.lower()}.png"
    fig.savefig(out, bbox_inches="tight")
    print("wrote", out)
