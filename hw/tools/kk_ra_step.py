"""STEPs of the Molex 22-05-3041/3031 into the KiCad footprint frame: origin at the tails' pin 1, board top at Z = 0,
housing above it, the plug entering toward -X, the pin row along +Y (pin i at Y = 2.54 i)."""
import sys
from build123d import import_step, export_step, Axis, Location, Plane, Vector, Pos
src, dst, pin1_x = sys.argv[1], sys.argv[2], float(sys.argv[3])
s = import_step(src)
s = s.rotate(Axis((0, 0, 0), (1, -1, 0)), 180)  # (x, y, z) -> (-y, -x, -z)
s = Pos(-4.32, pin1_x, 3.18) * s
b = s.bounding_box()
print(dst.split('/')[-1], "x %.2f..%.2f y %.2f..%.2f z %.2f..%.2f" % (b.min.X, b.max.X, b.min.Y, b.max.Y, b.min.Z, b.max.Z))
export_step(s, dst)
