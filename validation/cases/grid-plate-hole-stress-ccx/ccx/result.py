"""Print the reference values of grid-plate-hole-stress-ccx from the job.dat of CalculiX (argument 1)."""
import sys

blocks = {}
name = None
for line in open(sys.argv[1]):
    words = line.split()
    if line.startswith(" displacements"):
        name = (words[0], words[words.index("set") + 1])
        blocks[name] = []
    elif line.startswith(" ") and words and name and words[0].isdigit():
        blocks[name].append([float(w) for w in words[1:]])
    elif words and not words[0].isdigit():
        name = None

for s in ("PAD_CORNER", "PLATE_FAR", "PLATE_NEAR"):
    u = blocks[("displacements", s)][0]
    print(" ".join(f"{s.lower()}.{c} {v:.6e}" for c, v in zip(("ux", "uy", "uz"), u)))
