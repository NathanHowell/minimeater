"""The air inside the duct, as the solver sees it (mm, assembly frame, flow along +x).

The fan's outlet annulus (the frame's bore around the motor hub, the last `FAN_STUB` mm of the fan), the duct's
contraction cone, the straight bore with the flap and its rod at an angle, and the intake tube from the socket's
shoulder on for `tube_d` diameters (so the flow can settle and the loss be read off the developed pressure line).
The fan's blades and struts are not modelled: the inlet is the annulus, with an optional swirl (case.py).
"""

from dataclasses import dataclass
from pathlib import Path

from build123d import Compound, Vector, export_stl

from .. import mock
from ..assembly import about_shaft
from ..geom import g
from ..params import P
from ..parts import flap, xcone, xcyl

FAN_STUB = 5.0  # the fan's outlet annulus ahead of its face: the hub's wake starts at the face


@dataclass(frozen=True)
class Stations:
    x_in: float
    x_fan: float
    x_bore: float
    x_tube: float
    x_out: float
    r_hub: float
    r_fan: float
    r_bore: float
    r_tube: float


def stations(tube_d: float) -> Stations:
    return Stations(x_in=g.fan_face_x - FAN_STUB, x_fan=g.fan_face_x, x_bore=g.straight_x0, x_tube=g.socket_x0,
                    x_out=g.socket_x0 + tube_d * P.tube.id, r_hub=P.fan.hub_d / 2, r_fan=P.fan.aperture_d / 2,
                    r_bore=g.bore_r, r_tube=P.tube.id / 2)


def fluid(flap_deg: float, tube_d: float):
    """The air volume with the flap at `flap_deg` (0 open, 90 closed)."""
    s = stations(tube_d)
    air = xcyl(s.r_fan, s.x_in, s.x_fan + 0.01) - xcyl(s.r_hub, s.x_in - 1, s.x_fan)
    air += xcone(s.r_fan, s.r_bore, s.x_fan, s.x_bore)
    air += xcyl(s.r_bore, s.x_bore - 0.01, s.x_tube)
    air += xcyl(s.r_tube, s.x_tube - 0.01, s.x_out)
    air -= about_shaft(flap(), flap_deg)
    air -= about_shaft(mock.rod(), flap_deg)
    return air


def patches(air, tube_d: float) -> dict[str, list]:
    """The faces of `air` by patch: the inlet annulus, the outlet disc, the flap and its rod, the duct's walls
    (the fan's hub, the cone, the bore and the shoulder) and the tube's."""
    s = stations(tube_d)
    out = {"inlet": [], "outlet": [], "flap": [], "duct": [], "tube": []}
    for f in air.faces():
        b = f.bounding_box()
        flat = b.size.X < 1e-6  # a face square to the axis
        if flat and abs(b.min.X - s.x_in) < 1e-6:
            out["inlet"].append(f)
        elif flat and abs(b.min.X - s.x_out) < 1e-6:
            out["outlet"].append(f)
        elif _on_flap(f, s):
            out["flap"].append(f)
        elif not flat and b.min.X > s.x_tube - 1e-6:
            out["tube"].append(f)
        else:
            out["duct"].append(f)
    return out


def _on_flap(f, s: Stations) -> bool:
    """The flap's and the rod's faces lie inside the bore, near the shaft; the bore's own wall is at r_bore."""
    pts = [f.position_at(u, v) for u in (0.2, 0.5, 0.8) for v in (0.2, 0.5, 0.8)]
    near = all(abs(p.X - g.flap_x) < s.r_bore for p in pts)
    inside = all((p.Y**2 + p.Z**2) ** 0.5 < s.r_bore - 0.01 for p in pts)
    return near and inside


def write_stl(out: Path, flap_deg: float, tube_d: float) -> dict[str, int]:
    """One STL per patch in `out` (mm). Returns the face count of each."""
    air = fluid(flap_deg, tube_d)
    if len(air.solids()) != 1:
        raise ValueError(f"the air volume is {len(air.solids())} solids at {flap_deg} deg")
    out.mkdir(parents=True, exist_ok=True)
    groups = patches(air, tube_d)
    for name, faces in groups.items():
        if not faces:
            raise ValueError(f"no faces for patch {name}")
        export_stl(Compound(faces), str(out / f"{name}.stl"), tolerance=0.01, angular_tolerance=0.05)
    return {k: len(v) for k, v in groups.items()}


def inside_point(tube_d: float) -> Vector:
    """A point in the air, off every cell face of the background mesh: in the bore, between the cone and the flap."""
    s = stations(tube_d)
    return Vector((s.x_bore + g.flap_x - s.r_bore) / 2 + 0.137, 3.113, 5.071)
