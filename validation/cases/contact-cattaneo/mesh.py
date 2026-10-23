"""Write mesh.inp: a cylindrical steel pad on a steel block, in plane strain, with CPE4 elements.

Usage: python3 mesh.py [--size H] [--grow G] [-o FILE]

The block is -20 <= x <= 20, -20 <= y <= 0. The pad is -12 <= x <= 12, from the arc
y = R - sqrt(R^2 - x^2) (R = 20 mm) up to y = 20. The 2 bodies touch at (0, 0).
Near the contact (|x| <= 1.4, and 0.6 mm deep in each body) the elements are squares of size H.
Outside, each element is G times larger than the element before it. The vertical mesh lines of
the 2 bodies are the same lines for |x| <= 12.

The sets: BLOCK and PAD (elements), BOTTOM (the nodes of y = -20), TOP (the nodes of y = 20),
LEFT and RIGHT (the nodes of TOP at x = -12 and x = 12), CENTRE (the 2 elements of the block at
(0, 0)). The surfaces: PADS (the pad faces of |x| <= 1.4) and BLOCKS (the block faces of
|x| <= 2). The equations keep the top of the pad straight and give it the length 24 mm: each
node of TOP has U1 of LEFT, and U2 on the line between LEFT and RIGHT. The top can turn.
"""

import argparse
import math

R, L, X, HEIGHT = 20.0, 20.0, 12.0, 20.0
CORE, DEPTH = 1.4, 0.6

p = argparse.ArgumentParser()
p.add_argument("--size", type=float, default=0.04, help="element size H near the contact")
p.add_argument("--grow", type=float, default=1.3, help="size ratio G of 2 elements outside")
p.add_argument("-o", default="mesh.inp")
args = p.parse_args()
h, g = args.size, args.grow


def offsets(core, length):
    """0, h, 2h ... core, then the larger steps up to length."""
    n = round(core / h)
    out = [core * k / n for k in range(n + 1)]
    steps, s = [], h
    while sum(steps) + s * g <= length - core:
        s *= g
        steps.append(s)
    scale = (length - core) / sum(steps)
    for s in steps:
        out.append(out[-1] + s * scale)
    out[-1] = length
    return out


def axis(length):
    right = offsets(CORE, length)
    return [-x for x in reversed(right[1:])] + right


nodes, elements = [], []  # (x, y), (n1, n2, n3, n4)


def grid(xs, ys):
    """Nodes on the lines xs x ys(x) and the CPE4 elements between them. ys(x) goes up."""
    first = len(nodes) + 1
    for j in range(len(ys(0.0))):
        for x in xs:
            nodes.append((x, ys(x)[j]))
    nx = len(xs)
    node = lambda i, j: first + j * nx + i
    for j in range(len(ys(0.0)) - 1):
        for i in range(nx - 1):
            elements.append((node(i, j), node(i + 1, j), node(i + 1, j + 1), node(i, j + 1)))
    return node


down = offsets(DEPTH, HEIGHT)
xb, xp = axis(L), axis(X)
block = grid(xb, lambda x: [-d for d in reversed(down)])
n_block = len(elements)


def pad_ys(x):
    bottom = R - math.sqrt(R * R - x * x)
    return [bottom + d * (HEIGHT - bottom) / HEIGHT for d in down]


pad = grid(xp, pad_ys)
top = len(down) - 1  # the row of the top of the pad
first_top = n_block - (len(xb) - 1)  # the first element of the top row of the block, from 0
mid_b = [(xb[i] + xb[i + 1]) / 2 for i in range(len(xb) - 1)]
mid_p = [(xp[i] + xp[i + 1]) / 2 for i in range(len(xp) - 1)]


def lines(ids, per=16):
    ids = list(ids)
    return "\n".join(", ".join(str(i) for i in ids[k : k + per]) for k in range(0, len(ids), per))


out = [f"** Written by mesh.py --size {h} --grow {g}", "*NODE"]
out += [f"{k}, {x:.9g}, {y:.9g}" for k, (x, y) in enumerate(nodes, 1)]
out.append("*ELEMENT, TYPE=CPE4, ELSET=BLOCK")
out += [f"{k}, {a}, {b}, {c}, {d}" for k, (a, b, c, d) in enumerate(elements[:n_block], 1)]
out.append("*ELEMENT, TYPE=CPE4, ELSET=PAD")
out += [f"{k}, {a}, {b}, {c}, {d}" for k, (a, b, c, d) in enumerate(elements[n_block:], n_block + 1)]
out += ["*NSET, NSET=BOTTOM", lines(block(i, 0) for i in range(len(xb)))]
out += ["*NSET, NSET=TOP", lines(pad(i, top) for i in range(len(xp)))]
left, right = pad(0, top), pad(len(xp) - 1, top)
out += ["*NSET, NSET=LEFT", str(left), "*NSET, NSET=RIGHT", str(right)]
out += ["*ELSET, ELSET=CENTRE", lines(first_top + i + 1 for i, x in enumerate(mid_b) if abs(x) < h)]
out.append("*SURFACE, NAME=PADS, TYPE=ELEMENT")
out += [f"{n_block + i + 1}, S1" for i, x in enumerate(mid_p) if abs(x) < CORE]
out.append("*SURFACE, NAME=BLOCKS, TYPE=ELEMENT")
out += [f"{first_top + i + 1}, S3" for i, x in enumerate(mid_b) if abs(x) < 2.0]
out.append("*EQUATION")
for i in range(1, len(xp)):
    n, t = pad(i, top), (xp[i] + X) / (2 * X)
    out += ["2", f"{n}, 1, 1.0, {left}, 1, -1.0"]
    if i < len(xp) - 1:
        out += ["3", f"{n}, 2, 1.0, {left}, 2, {t - 1:.12g}, {right}, 2, {-t:.12g}"]
open(args.o, "w").write("\n".join(out) + "\n")
print(f"{args.o}: {len(nodes)} nodes, {len(elements)} elements")
