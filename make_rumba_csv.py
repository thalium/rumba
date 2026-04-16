import csv
import time
from pathlib import Path
from tqdm import tqdm

from pyrumba import Expr

results_rumba = []

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


with open("rumba_res_2.csv", "w") as f:
    fieldnames = results_rumba[0].keys()
    writer = csv.DictWriter(f, fieldnames=fieldnames)
    writer.writeheader()
    writer.writerows(results_rumba)
