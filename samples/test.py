with open("./mbas.csv") as f:
    lines = f.readlines()

res = []

for line in lines[1:]:
    (lin, gt, poly) = line.split(",")
    res.append(gt + "," + lin + "\n")
    res.append(gt + "," + poly)

with open("./mbas.csv", "w") as f:
    f.writelines(res)
