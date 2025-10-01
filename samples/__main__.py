from pyrumba import Expr
from pathlib import Path
import random
import csv
import time
import tqdm

INPUT_FILE = Path(__file__).resolve().parent.joinpath("mbas.csv")
OUTPUT_FILE = Path(__file__).resolve().parent.joinpath("out.csv")

TEST_ITERATIONS = 100


def test_line(line: str):
    [gt, mba] = line.split(",")

    # Parse the expressions
    gt = Expr(gt)
    mba = Expr(mba)

    start = time.perf_counter()
    solution = mba.solve_poly()
    elapsed = time.perf_counter() - start

    # Test the IO equivilancy
    for _ in range(TEST_ITERATIONS):
        v0 = random.randint(0, 2**32 - 1)
        v1 = random.randint(0, 2**32 - 1)
        v2 = random.randint(0, 2**32 - 1)
        v3 = random.randint(0, 2**32 - 1)

        if solution.eval([v0, v1, v2, v3], 32) != gt.eval([v0, v1, v2, v3], 32):
            return (elapsed, False, None)

    size = (solution.size() - gt.size()) / gt.size()
    return (elapsed, True, size)


# Attempts to simplify each mba in a file, comparing the result to the ground truth
def run():
    with open(INPUT_FILE) as f, open(OUTPUT_FILE, "w", encoding="utf-8") as csvfile:
        lines = f.readlines()

        writer = csv.writer(csvfile)
        writer.writerow(["line_number", "exec_time", "success", "size"])

        # Skip the title
        lines = lines[1:]

        for i, line in tqdm.tqdm(enumerate(lines, start=2), total=len(lines)):
            (elapsed, success, size) = test_line(line)
            writer.writerow([i, elapsed, success, size])


if __name__ == "__main__":
    run()
