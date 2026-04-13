import pandas as pd
import matplotlib.pyplot as plt
from matplotlib.patches import Patch

TEAL = "#298c8c"
GOLD = "#f1a226"

# Load CSVs
df_gamba = pd.read_csv("./build/gamba_res.csv")
df_rumba = pd.read_csv("./build/rumba_res.csv")

# Plot
plt.figure()
plt.scatter(df_gamba["size"], df_gamba["time"], s=5, alpha=0.05, color=TEAL)
plt.scatter(df_rumba["size"], df_rumba["time"], s=5, alpha=0.05, color=GOLD)

# Log scale (pick what you need)
plt.xscale("log")  # log time
plt.yscale("log")  # log size

plt.xlabel("Simplification Time (ms)")
plt.ylabel("Expression Size")
legend_elements = [
    Patch(facecolor=TEAL, label="Gamba"),
    Patch(facecolor=GOLD, label="Rumba"),
]
plt.legend(handles=legend_elements)

plt.grid(True, which="major", linewidth=0.5)

plt.show()
