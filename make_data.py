import csv
import time
from pathlib import Path
from tqdm import tqdm
import sys
import os

from pyrumba import Expr

GAMBA_DIR = "./GAMBA"

sys.path.insert(0, os.path.join(GAMBA_DIR, "src"))
from simplify_general import GeneralSimplifier

results_rumba = []
results_gamba = []

directory = Path("./third_party/dataset")

for path in directory.iterdir():
    if not path.is_file():
        continue
    if str(path).endswith("LICENSE.md") or str(path).endswith(".gitignore"):
        continue

    print(f"\n{path}")

    with open(path) as f:
        for row in tqdm(list(csv.reader(f))):
            e = Expr(row[0])
            size = e.size()

            start = time.perf_counter_ns()
            e.solve(64)
            dt = (time.perf_counter_ns() - start) / 1000

            results_rumba.append({"time": dt, "size": size})

            simplifier = GeneralSimplifier(64, False, None)
            start = time.perf_counter_ns()
            r = simplifier.simplify(row[0])
            dt = (time.perf_counter_ns() - start) / 1000

            results_gamba.append({"time": dt, "size": size})


with open("gamba_res.csv", "w") as f:
    fieldnames = results_gamba[0].keys()
    writer = csv.DictWriter(f, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(results_gamba)

with open("rumba_res.csv", "w") as f:
    fieldnames = results_rumba[0].keys()
    writer = csv.DictWriter(f, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(results_rumba)
