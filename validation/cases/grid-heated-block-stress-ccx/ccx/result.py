"""Print the reference values of grid-heated-block-stress-ccx from the job.dat of CalculiX (argument 1)."""
import sys

blocks = {}
name = None
for line in open(sys.argv[1]):
    words = line.split()
    if line.startswith(" displacements") or line.startswith(" temperatures"):
        name = (words[0], words[words.index("set") + 1])
        blocks[name] = []
    elif line.startswith(" ") and words and name and words[0].isdigit():
        blocks[name].append([float(w) for w in words[1:]])
    elif words and not words[0].isdigit():
        name = None


def u(s):
    return blocks[("displacements", s)][0]


uz = [v[2] for v in blocks[("displacements", "BASE")]]
print(f"block_top.uz {u('BLOCK_TOP')[2]:.6e}")
print(f"plate_corner.ux {u('PLATE_CORNER')[0]:.6e}")
print(f"plate_corner.uz {u('PLATE_CORNER')[2]:.6e}")
print(f"plate_edge.ux {u('PLATE_EDGE')[0]:.6e}")
print(f"plate_edge.uz {u('PLATE_EDGE')[2]:.6e}")
print(f"bottom.range {max(uz) - min(uz):.6e}")
