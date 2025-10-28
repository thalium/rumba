#!/usr/bin/python3

from enum import Enum
import inspect
import os
import sys
import hashlib
import time
import numpy as np

from tqdm import tqdm


from pyrumba import Expr
import random

from datasets import datasets, classify_dataset

currentdir = os.path.dirname(os.path.abspath(inspect.getfile(inspect.currentframe())))
parentdir = os.path.dirname(currentdir)
sys.path.insert(0, os.path.join(parentdir, "src"))


# unique experiments (hash expression)
experiments = {}


class Experiment:
    def __init__(self, expr, gt, dsName, bits):
        self.expression = expr
        self.groundtruth = gt
        self.dataset_names = {dsName}
        self.properties = set()
        self.solved = set()
        self.bitCount = bits

    @staticmethod
    def hash(expr, gt):
        return hashlib.sha256((expr + gt).encode("utf-8")).hexdigest()


def check_print_error(ex, verbosity, idx, lineno, expr):
    if verbosity >= 1:
        print('ERR%d(line %d): "%s" -- "%s"' % (idx, lineno, expr, str(ex)))
        if verbosity >= 4:
            import traceback

            traceback.print_tb(ex.__traceback__)


def check_results(e1, e2):
    for _ in range(1000):
        vars = [random.randint(0, 10000) for _ in range(16)]
        if e1.eval(vars, 32) != e2.eval(vars, 32):
            return False
    return True


def process_dataset(
    fname,
    bitCount,
    verbosity=0,
    limit=-1,
    check=False,
    useZ3=False,
):
    f = open(os.path.join(currentdir, "datasets", fname), "rt")

    lineno = 0  # line number in file
    ok = 0  # expected results (completely equal to ground truth)
    okz = 0  # not ok, but verified using GAMBA
    z3 = 0  # not okz, but verified using Z3
    to = 0  # timed out
    ng = 0  # non-verifiable results
    nc = 0  # unchecked expressions which do not meet requirements
    err = 0  # error

    dur = 0
    cnt = 0
    timing = []

    for line in tqdm(f.readlines()):
        lineno += 1

        # Allow to limit processing of dataset.
        if limit > 0 and ok + okz + z3 + to + ng + nc + err >= limit:
            break

        # Ignore comments.
        if line.startswith("#"):
            continue

        # Split line: expression, ground truth [, whatever, ...]
        try:
            e, gt = line.split(",")[:2]
        except ValueError as ex:
            err += 1
            if verbosity >= 1:
                print('ERR0(line %d): "%s" -- "%s"' % (lineno, line.strip(), str(ex)))
            continue

        e = e.strip()
        gt = gt.strip()

        if verbosity >= 5:
            print("\n" + str(lineno) + ": Expression: " + e)

        h = Experiment.hash(e, gt)
        if h in experiments:
            exp = experiments[h]
            exp.dataset_names.add("%s:%d" % (ds.name, lineno))
        else:
            exp = Experiment(e, gt, "%s:%d" % (ds.name, lineno), ds.bitCount)
            experiments[h] = exp

        # Run MBA simplifier.
        try:
            start = time.perf_counter()
            r = Expr(e).solve_non_poly(bitCount)

            cnt += 1
            d = time.perf_counter() - start
            timing.append(d)
            dur += d
        except (Exception, BaseException) as ex:
            err += 1
            check_print_error(ex, verbosity, 3, lineno, e)
            continue

        # Did we make a mistake ?
        if not check_results(r, Expr(gt)):
            err += 1
            print(f"Verification failed for line {lineno}: {e} => {r} != {gt}")
            check_print_error("Semantics mismatch", verbosity, 4, lineno, gt)
            continue

        # Simplify the ground truth in order to have some standard format for comparison.
        try:
            rgt = Expr(gt).solve_non_poly(bitCount)
        except (Exception, BaseException) as ex:
            err += 1
            check_print_error(ex, verbosity, 4, lineno, gt)
            continue

        if verbosity >= 5:
            print(f"Result: {r}")
            print(f"Groundtruth: {gt} => {rgt}")

        # We have got the expected result.
        if str(r.simplify()) == str(rgt.simplify()):
            check_results(r, Expr(gt))
            exp.solved.add("np")
            ok += 1
            continue

        # Finally neither simplification nor verification were successful.
        ng += 1
        if verbosity >= 2:
            print('NG(line %d): "%s": "%s" <=> "%s"' % (lineno, e, r, rgt))

    # Print timing statistics
    if cnt > 0:
        avg = (1.0 * dur) / cnt
        print("  * average duration: %.3f s / %d" % (avg, cnt))
        q = np.quantile(timing, [0.0, 0.25, 0.5, 0.75, 1.0])
        print(
            f"  * quantiles: [{q[0]:.0e} {q[1]:.0e} {q[2]:.0e} {q[3]:.0e} {q[4]:.0e}]"
        )

    return ok, okz, z3, to, ng, nc, err


def print_usage():
    print("Usage: python3 tests.py")
    print("")
    print("Command line options:")
    print(
        "    --check (-c):      enable verification of simplification results on 4-bit inputs"
    )
    print("    --classify (-f):   additionally classify the MBAs")
    print("    --dataset (-d):    specify dataset (0-5, default: all)")
    print("    --help (-h):       print usage")
    print("    --linear (-l):     run the linear simplifier only (default: general)")
    print(
        "    --number (-n):     limit processing to first n expressions per dataset (default: all)"
    )
    print(
        "    --verbosity (-v):  specify verbosity: 0 (default): summary / 1: errors / 2: errors and incorrect / 3: errors and not checked / 4: debug / 5: each line"
    )
    print("    --z3 (-z):         enable check for semantic equivalence using Z3")


if __name__ == "__main__":
    argc = len(sys.argv)
    verbosity = 0
    indexList = []
    nmax = -1
    check = False
    useZ3 = False
    classify = False

    i = 0
    while i < argc - 1:
        i = i + 1
        arg = sys.argv[i]

        if arg in ["--help", "-h"]:
            print_usage()
            sys.exit(0)

        elif arg in ["--verbosity", "-v"]:
            i = i + 1
            if i == argc:
                print("Error: No verbosity given!")
                print_usage()
                sys.exit(1)

            try:
                verbosity = int(sys.argv[i])
            except ValueError:
                print('Error: Invalid verbosity value "%s" given!' % (sys.argv[i]))
                print_usage()
                sys.exit(1)

        elif arg in ["--dataset", "-d"]:
            i = i + 1
            if i == argc:
                print("Error: No dataset indices given!")
                print_usage()
                sys.exit(1)

            try:
                indexList = [int(idx) for idx in sys.argv[i].split(",")]
            except ValueError:
                print('Error: Invalid dataset indices "%s" given!' % (sys.argv[i]))
                print_usage()
                sys.exit(1)

        elif arg in ["--number", "-n"]:
            i = i + 1
            if i == argc:
                print("Error: No number/limit given!")
                print_usage()
                sys.exit(1)

            try:
                nmax = int(sys.argv[i])
            except ValueError:
                print('Error: Invalid number/limit "%s" given!' % (sys.argv[i]))
                print_usage()
                sys.exit(1)

        elif arg in ["--check", "-c"]:
            check = True
        elif arg in ["--z3", "-z"]:
            z3 = True
        elif arg in ["--classify", "-f"]:
            classify = True

        else:
            print("Error: Unknown command line option!")
            print_usage()
            sys.exit(1)

    if indexList:
        datasets = [datasets[i] for i in indexList]
    print(f"Got {len(datasets)} datasets to test...")

    for ds in datasets:
        if classify:
            path = os.path.join(currentdir, "datasets", ds.fname)
            (
                count,
                linear,
                nonlinear,
                mixed,
                bitwise,
                vnumber_min,
                vnumber_max,
                vnumber_avg,
                alt_min,
                alt_max,
                alt_avg,
                strlen_min,
                strlen_max,
                strlen_avg,
                nodes_min,
                nodes_max,
                nodes_avg,
            ) = classify_dataset(path, ds.bitCount)
            print(
                "------------- %s: %d bitwise, %d linear, %d nonlinear, %d mixed / %d (%d to %d vars, %d to %d nodes / %2.f, %d to %d alt / %.2f)"
                % (
                    ds,
                    bitwise,
                    linear,
                    nonlinear,
                    mixed,
                    count,
                    vnumber_min,
                    vnumber_max,
                    nodes_min,
                    nodes_max,
                    nodes_avg,
                    alt_min,
                    alt_max,
                    alt_avg,
                )
            )
        else:
            print(f"------------- {ds}")

        ok, okz, z3, to, ng, nc, err = process_dataset(
            ds.fname, ds.bitCount, verbosity, nmax, check, useZ3
        )
        print(f"OK {ok}, OKZ {okz}, Z3 {z3}, TO {to}, NG {ng}, NC {nc}, err {err}")

    total = 0
    solved = unsolved = 0
    for h, exp in experiments.items():
        if len(exp.dataset_names) > 1:
            total += len(exp.dataset_names)
        else:
            total += 1

        if exp.solved:
            solved += 1
        else:
            unsolved += 1

    print("Got %d unique experiments, %d total" % (len(experiments), total))
    print("%d solved, %d unsolved" % (solved, unsolved))

    print()
    print("Unsolved:")
    for h, exp in experiments.items():
        if not exp.solved:
            r = Expr(exp.expression).solve_non_poly(exp.bitCount)

            rgt = Expr(exp.groundtruth).solve_non_poly(exp.bitCount)

            print(f"Expression: {exp.expression}")
            print(f"Result: {r}")
            print(f"Groundtruth: {exp.groundtruth} => {rgt}")
            print()

    sys.exit(0)
