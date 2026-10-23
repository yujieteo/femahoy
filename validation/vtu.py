"""Read a step file of `fh run` (a VTU file with raw appended data) with only the Python standard
library. The curves.py of a validation case uses it (scripts/validation-reports.py):

    sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
    import vtu
    step = vtu.read(folder / "step-1.vtu")
    step.points[i], step.point["NT"][i], step.cell["p"][k]

A field with 1 component gives 1 number for each point or cell, a field with more components a
tuple.
"""

import re
import struct
from pathlib import Path

CODES = {"Float64": "d", "Float32": "f", "Int64": "q", "Int32": "i", "UInt8": "B"}


class Step:
    def __init__(self, points, point, cell, connectivity, offsets):
        self.points = points
        self.point = point
        self.cell = cell
        self.connectivity = connectivity
        self.offsets = offsets

    def nodes(self, k):
        """The points of the cell k."""
        start = self.offsets[k - 1] if k else 0
        return self.connectivity[start : self.offsets[k]]


def read(path):
    data = Path(path).read_bytes()
    at = data.index(b'<AppendedData encoding="raw">')
    head = data[:at].decode()
    raw = data.index(b"_", at) + 1
    size = "<Q" if 'header_type="UInt64"' in head else "<I"
    arrays = {}
    for m in re.finditer(r"<DataArray ([^>]*)/>", head):
        a = dict(re.findall(r'(\w+)="([^"]*)"', m[1]))
        offset = raw + int(a["offset"])
        n = struct.unpack_from(size, data, offset)[0]
        code = CODES[a["type"]]
        values = struct.unpack_from(f"<{n // struct.calcsize(code)}{code}", data, offset + struct.calcsize(size))
        c = int(a.get("NumberOfComponents", "1"))
        # The section of the array: <Points>, <Cells>, <PointData> or <CellData>.
        section = max(("<Points", "<Cells", "<PointData", "<CellData"), key=lambda t: head.rfind(t, 0, m.start()))
        arrays[section, a["Name"]] = list(values) if c == 1 else [values[i : i + c] for i in range(0, len(values), c)]
    pick = lambda section: {name: v for (s, name), v in arrays.items() if s == section}
    cells = pick("<Cells")
    return Step(arrays["<Points", "Points"], pick("<PointData"), pick("<CellData"), cells["connectivity"], cells["offsets"])
