"""Wavey next to the round-robin participants (Brinkmann et al. 2019,
Aspöck 2020 Figs. 4.12 / 4.18): EDT and T20 averaged over ten RIRs,
125 Hz - 4 kHz, measured with the ±5 % JND, markers A-F as in the paper.

    python3 docs/validation/plot_roundrobin.py
"""
import csv
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt

sys.path.insert(0, "docs/validation")
from roundrobin_digitized import BANDS, CR2, CR4

MARK = {
    "A": dict(marker="+", color="#0072bd", ms=7, mew=1.5, ls="none"),
    "B": dict(marker="o", color="#3cb044", ms=4, ls="none"),
    "C": dict(marker="D", mfc="none", color="#edb120", ms=5, mew=1.2, ls="none"),
    "D": dict(marker="v", mfc="none", color="#1fc8c8", ms=6, mew=1.2, ls="none"),
    "E": dict(marker="*", color="#7e2f8e", ms=8, ls="none"),
    "F": dict(marker="s", mfc="none", color="#d95319", ms=5, mew=1.2, ls="none"),
}


def wavey(path):
    out = {}
    for r in csv.reader(open(path)):
        if r[0] in ("Wavey", "Measured") and r[1] in ("T20", "EDT"):
            out.setdefault(r[1], {})[r[0]] = [float(x) for x in r[3:9]]
    return out


def jnd_err(sim, meas):
    return sum(abs(s - m) / (0.05 * m) for s, m in zip(sim, meas)) / len(sim)


def figure(rr, w, title, out, top):
    fig, axes = plt.subplots(1, 2, figsize=(9.6, 4.2), dpi=160)
    x = list(range(len(BANDS)))
    for ax, p in zip(axes, ("EDT", "T20")):
        meas = rr[p]["Measured"]
        ax.plot(x, meas, color="black", lw=1.6, marker="x", label="Measurement", zorder=5)
        ax.plot(x, [m * 1.05 for m in meas], color="#444", ls="--", lw=0.8)
        ax.plot(x, [m * 0.95 for m in meas], color="#444", ls="--", lw=0.8)
        for i, t in enumerate("ABCDEF"):
            err = jnd_err(rr[p][t], meas)
            ax.plot([v + 0.06 * (i - 2.5) for v in x], rr[p][t], label=f"{t} ({err:.1f} JND)", **MARK[t])
        werr = jnd_err(w[p]["Wavey"], w[p]["Measured"])
        ax.plot(x, w[p]["Wavey"], color="#ff7f0e", lw=2.4, marker="o", ms=4, label=f"Wavey ({werr:.1f} JND)", zorder=6)
        ax.set_xticks(x)
        ax.set_xticklabels(["125", "250", "500", "1k", "2k", "4k"])
        ax.set_xlabel("Center frequencies in Hz")
        ax.set_ylabel(f"{p} in s")
        ax.set_ylim(0, top)
        ax.grid(True, color="#e6e6e6", lw=0.6)
        ax.legend(fontsize=7, loc="lower left", ncol=2)
    fig.suptitle(title, fontsize=10)
    fig.tight_layout()
    fig.savefig(out, bbox_inches="tight")
    print("wrote", out)


figure(CR2, wavey("docs/validation/bras-cr2.csv"), "Scene 9 (CR2): round-robin participants A-F and Wavey", "docs/validation/roundrobin-cr2.png", 3.0)
figure(CR4, wavey("docs/validation/bras-cr4-area.csv"), "Scene 11 (CR4): round-robin participants A-F and Wavey (absorption areas matched to the BRAS fit)", "docs/validation/roundrobin-cr4.png", 4.0)
