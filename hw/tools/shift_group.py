"""Move a group of parts and the copper among them: `shift_group.py <in> <out> <dx> <dy> <refs>` (KiCad's python).

The footprints in `refs` (comma separated) move by (dx, dy) mm. Tracks and vias of the group's nets lying wholly
inside its region (the union of its courtyards grown by 0.3 mm) move with them; a track with one end inside has that
end moved, so every joint stays joined. The stretched tracks are left for `routeall.py --repair` / `tidy.py` to
straighten; run DRC."""

import sys

sys.path.insert(0, "/Applications/KiCad/KiCad.app/Contents/Frameworks/Python.framework/Versions/Current/lib/python3.9/site-packages")
import pcbnew

F, M = lambda v: pcbnew.FromMM(float(v)), pcbnew.ToMM


def shift(b, refs, dx, dy):
    """Returns (tracks/vias moved, tracks stretched)."""
    fps = [b.FindFootprintByReference(r) for r in refs]
    assert all(fps), refs
    boxes = []
    for f in fps:
        c = f.GetCourtyard(pcbnew.F_CrtYd)
        bb = c.BBox() if c.OutlineCount() else f.GetBoundingBox(False, False)
        boxes.append((M(bb.GetX()) - 0.3, M(bb.GetY()) - 0.3, M(bb.GetRight()) + 0.3, M(bb.GetBottom()) + 0.3))
    nets = {p.GetNetCode() for f in fps for p in f.Pads() if p.GetNetCode()}
    inside = lambda p: any(x0 <= M(p.x) <= x1 and y0 <= M(p.y) <= y1 for x0, y0, x1, y1 in boxes)
    d = pcbnew.VECTOR2I(F(dx), F(dy))
    moved = stretched = 0
    for t in b.GetTracks():
        if t.GetNetCode() not in nets:  # passing through, not joined to the group
            continue
        if t.Type() == pcbnew.PCB_VIA_T:
            if inside(t.GetPosition()):
                t.SetPosition(t.GetPosition() + d)
                moved += 1
            continue
        a, e = inside(t.GetStart()), inside(t.GetEnd())
        if a and e:
            t.Move(d)
            moved += 1
        elif a:
            t.SetStart(t.GetStart() + d)
            stretched += 1
        elif e:
            t.SetEnd(t.GetEnd() + d)
            stretched += 1
    for f in fps:
        f.Move(d)
    return moved, stretched


def move_to(b, ref, x, y):
    """`shift` one part so its origin lands on (x, y) mm."""
    p = b.FindFootprintByReference(ref).GetPosition()
    return shift(b, [ref], x - M(p.x), y - M(p.y))


if __name__ == "__main__":
    b = pcbnew.LoadBoard(sys.argv[1])
    refs = sys.argv[5].split(",")
    moved, stretched = shift(b, refs, float(sys.argv[3]), float(sys.argv[4]))
    print(f"{len(refs)} parts moved; {moved} tracks/vias moved, {stretched} stretched")
    pcbnew.SaveBoard(sys.argv[2], b)
