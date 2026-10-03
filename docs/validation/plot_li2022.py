"""Wave-based solvers on BRAS CR2, 63–250 Hz, as in Li et al. 2022
(Appl. Acoust. 191, 108662, Table 1, scenario e: five frequency-dependent
materials, fitted BRAS coefficients). Wavey's wave solver alone (FDTD
impulse responses, `bras_wave` with a 700 Hz grid) and the full hybrid,
each against the measurement it was compared with.

python3 docs/validation/plot_li2022.py  ->  docs/validation/li2022-cr2.png
"""
import os
import matplotlib.pyplot as plt
import numpy as np

BANDS = [63, 125, 250]
PARAMS = ["T20", "EDT", "C80", "D50"]
UNIT = {"T20": "s", "EDT": "s", "C80": "dB", "D50": "–"}
# Li et al. 2022, Table 1 (measurement: their RIR selection).
li_meas = dict(T20=[1.74, 1.35, 1.63], EDT=[1.54, 1.40, 1.52], C80=[3.9, 2.8, 1.0], D50=[0.580, 0.443, 0.402])
li = {
    "FMBEM (Li 2022)": dict(T20=[2.30, 2.03, 1.96], EDT=[2.11, 1.86, 1.92], C80=[0.7, -0.2, -0.3], D50=[0.445, 0.356, 0.351]),
    "FDTD (Li 2022)": dict(T20=[2.22, 1.88, 1.89], EDT=[2.06, 1.76, 2.02], C80=[1.1, 0.3, -0.5], D50=[0.462, 0.377, 0.350]),
}
# Wavey (both sources, R1–R5; measurement from the same BRAS RIRs via wavey_solver::analysis).
w_meas = dict(T20=[1.595, 1.353, 1.647], EDT=[1.380, 1.395, 1.453], C80=[3.262, 1.483, 1.144], D50=[0.542, 0.363, 0.429])
wavey = {
    "Wavey wave solver": dict(T20=[1.678, 1.741, 1.716], EDT=[1.797, 1.743, 1.629], C80=[1.756, -0.573, -0.442], D50=[0.466, 0.310, 0.356]),
    "Wavey hybrid": dict(T20=[1.519, 1.420, 1.593], EDT=[1.519, 1.403, 1.598], C80=[0.410, 0.812, 0.239], D50=[0.372, 0.392, 0.369]),
}


def jnd(p, s, m):
    s, m = np.array(s), np.array(m)
    if p in ("T20", "EDT"):
        return np.abs(s - m) / (0.05 * m)
    return np.abs(s - m) / (0.05 if p == "D50" else 1.0)


fig, axes = plt.subplots(1, 5, figsize=(19, 4.2), gridspec_kw=dict(width_ratios=[1, 1, 1, 1, 1.1]))
colors = {"FMBEM (Li 2022)": "#d9822b", "FDTD (Li 2022)": "#444444", "Wavey wave solver": "#1f77b4", "Wavey hybrid": "#2ca02c"}
x = np.arange(3)
means = {}
for ax, p in zip(axes, PARAMS):
    ax.plot(x, li_meas[p], "k--", marker="s", mfc="none", label="Measured (Li 2022)")
    ax.plot(x, w_meas[p], "k-", marker="o", label="Measured (Wavey analysis)")
    for name, t in {**li, **wavey}.items():
        ax.plot(x, t[p], marker="^" if "Li" in name else "D", color=colors[name], label=name)
    ax.set_xticks(x, [str(b) for b in BANDS])
    ax.set_xlabel("Octave band (Hz)")
    ax.set_title(f"{p} ({UNIT[p]})")
    ax.grid(alpha=0.3)
for name, t in li.items():
    means[name] = np.mean([jnd(p, t[p], li_meas[p]).mean() for p in PARAMS])
for name, t in wavey.items():
    means[name] = np.mean([jnd(p, t[p], w_meas[p]).mean() for p in PARAMS])
ax = axes[4]
names = list(means)
ax.barh(names, [means[n] for n in names], color=[colors[n] for n in names])
for i, n in enumerate(names):
    ax.text(means[n] + 0.05, i, f"{means[n]:.2f}", va="center")
ax.invert_yaxis()
ax.set_xlabel("Mean error (JND), 63–250 Hz")
ax.set_title("Error vs. measurement")
axes[0].legend(fontsize=7, loc="upper right")
fig.suptitle("BRAS CR2, wave-based solvers (Li et al. 2022, Table 1, scenario e) and Wavey")
fig.tight_layout()
out = os.path.join(os.path.dirname(__file__), "li2022-cr2.png")
fig.savefig(out, dpi=130)
print(out)
for n in names:
    print(f"{n:20} {means[n]:.2f}")
