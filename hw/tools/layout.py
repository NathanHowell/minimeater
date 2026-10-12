import json, sys, itertools
D = json.load(open('/tmp/minimeat-place/parts.json'))
W, H = 46.0, 54.0
GAP = 0.25
pos = {}   # ref -> (x, y, rot)

def box(ref, x, y, rot):
    x0, y0, x1, y1 = D[ref]['crt']
    # rotate courtyard about origin (KiCad: positive = ccw on screen, y down); only 0/90/180/270
    pts = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    import math
    a = math.radians(rot); c, s = round(math.cos(a)), round(math.sin(a))
    q = [(px * c + py * s, -px * s + py * c) for px, py in pts]   # screen-ccw with y down
    xs = [p[0] for p in q]; ys = [p[1] for p in q]
    return (x + min(xs), y + min(ys), x + max(xs), y + max(ys))

def put(ref, x, y, rot=0):
    pos[ref] = (round(x, 3), round(y, 3), rot)

def dims(ref, rot):
    b = box(ref, 0, 0, rot); return b[2] - b[0], b[3] - b[1], b

def col(refs, xc, y, rot=0, gap=GAP, align='c'):
    """stack top to bottom at centre x (courtyard-centred); returns next y"""
    for r in refs:
        w, h, b = dims(r, rot)
        put(r, xc - (b[0] + b[2]) / 2, y - b[1], rot)
        y += h + gap
    return y

def row(refs, x, yc, rot=0, gap=GAP):
    for r in refs:
        w, h, b = dims(r, rot)
        put(r, x - b[0], yc - (b[1] + b[3]) / 2, rot)
        x += w + gap
    return x

def at(ref, x0, y0, rot=0):
    """place so the courtyard's top-left is (x0, y0)"""
    w, h, b = dims(ref, rot)
    put(ref, x0 - b[0], y0 - b[1], rot)

def centre(ref, cx, cy, rot=0):
    w, h, b = dims(ref, rot)
    put(ref, cx - (b[0] + b[2]) / 2, cy - (b[1] + b[3]) / 2, rot)

# ---------------- fixed: the connector row on the -y edge ----------------
put('J1', 5.7, 4.28, 180)
JX = (15.2, 23.9, 32.6, 41.3)
for j, x in zip(('J2', 'J3', 'J4', 'J5'), JX):
    put(j, x, 6.85, 0)
put('U1', 23.0, 40.95, 180)

# ---------------- left strip, x 0.5..12.7: USB protection, VBUS, buck ----------------
at('D1', 0.8, 10.0); at('F1', 6.0, 10.0)
at('U2', 0.8, 13.0); at('U3', 5.6, 13.0)
at('R1', 10.2, 12.9); at('R2', 10.2, 14.1)
at('C1', 0.8, 16.9); at('C2', 5.8, 16.9); at('C3', 9.5, 16.9)
at('U4', 0.8, 21.2); at('C4', 5.4, 21.2); at('C14', 9.1, 21.2)
at('L1', 0.8, 25.0); at('C5', 5.2, 23.7); at('C6', 8.9, 23.7)
at('R3', 0.8, 28.6); at('R4', 3.0, 28.6); at('C7', 5.2, 28.6)

# ---------------- headers on the rear edge, vertical ----------------
at('J6', 0.8, 30.2, 90)
at('J7', 0.8, 41.6, 90)
at('U10', 1.2, 51.5); at('C33', 4.2, 51.8)

# ---------------- load switches: column x 8.4..12.7 ----------------
at('U7', 8.4, 31.9); at('C12', 8.4, 35.7); at('C13', 8.4, 37.9); at('C17', 10.8, 37.9)
at('R6', 8.4, 39.3); at('R9', 10.6, 39.3)
at('U6', 8.4, 40.7); at('C10', 8.4, 44.5); at('C11', 8.4, 46.7); at('C16', 10.8, 46.7)
at('R5', 8.4, 48.1); at('R8', 10.6, 48.1)
at('C36', 8.2, 49.7)

# ---------------- middle band: ESD behind the jacks, then the ADC with its channel networks ----------------
at('U9', 26.0, 13.0)                                  # TPD4E05U06: the four tips come to it first
# reference rail, close to the ADC's REFP side
at('FB2', 14.6, 15.4); at('R18', 18.2, 15.4); at('C27', 21.6, 15.0); at('C28', 25.2, 15.4)
at('U8', 14.6, 18.8)
at('C24', 23.2, 18.8); at('C25', 23.2, 20.0); at('C26', 23.2, 21.2); at('R17', 23.2, 22.4)
# channel networks: 1k, 47k bias, 100 nF per column
for i, (r1, rb, cc) in enumerate((('R19', 'R23', 'C29'), ('R20', 'R24', 'C30'), ('R21', 'R25', 'C31'), ('R22', 'R26', 'C32'))):
    x0 = 25.6 + 3.4 * i
    at(r1, x0, 17.7); at(rb, x0, 19.1); at(cc, x0, 20.8)
# analog LDO, east
at('U5', 40.0, 17.2); at('FB1', 40.0, 21.0); at('C8', 40.0, 23.0); at('C9', 42.2, 23.0); at('C15', 43.4, 21.0)
at('U11', 14.6, 24.6); at('C34', 22.6, 24.6)
# header series parts, in the strip between the band and the module
x = 24.0
for r in ('R27', 'R28', 'R29', 'R30', 'C35', 'C37'):
    w, h, b = dims(r, 0); at(r, x, 28.7); x += w + 0.4
# test points
for i, r in enumerate(('TP1', 'TP2', 'TP3', 'TP4')): at(r, 26.0 + 3.0 * i, 24.8)

# ---------------- right strip: MCU support ----------------
at('C21', 33.8, 45.2); at('C22', 37.8, 45.2)
at('R10', 33.8, 43.4); at('C18', 36.0, 43.4); at('TP5', 33.6, 40.6)
at('C19', 34.2, 36.6); at('C20', 36.4, 36.6)
at('R12', 34.4, 52.2); centre('D2', 38.0, 53.0, 0)
at('SW1', 35.2, 30.2)
at('R13', 33.6, 34.5); at('C23', 35.6, 34.5)
at('R14', 41.8, 34.0); at('R15', 41.8, 35.4); at('R16', 41.8, 36.8); at('R11', 41.8, 38.2); at('R7', 41.8, 39.6)
at('TP6', 40.2, 42.4); at('TP7', 43.0, 42.4); at('TP8', 38.4, 40.0)

HOLES = {'H1': (10.6, 28.7), 'H2': (43.0, 29.8), 'H3': (43.0, 49.6)}
for h,(hx,hy) in HOLES.items():
    D[h] = {'crt': (-3.0,-3.0,3.0,3.0)}; put(h, hx, hy, 0)
missing = [r for r in D if r not in pos and r not in ('D','U')]
print('missing', missing)
# overlap check
boxes = {r: box(r, *pos[r]) for r in pos}
bad = []
for (a, ba), (b, bb) in itertools.combinations(boxes.items(), 2):
    if ba[0] < bb[2] - 0.01 and bb[0] < ba[2] - 0.01 and ba[1] < bb[3] - 0.01 and bb[1] < ba[3] - 0.01:
        bad.append((a, b))
outside = [r for r, b in boxes.items() if b[0] < -2 or b[1] < -0.01 or b[2] > W + 3 or b[3] > H + 3.1 and r != 'U1' or (r != 'U1' and b[3] > H - 0.3)]
print('overlaps', bad)
print('outside', [(r, [round(v, 2) for v in boxes[r]]) for r in outside if r not in ('J1',)])
json.dump({r: pos[r] for r in pos}, open('/tmp/minimeat-place/pos.json', 'w'))
# picture
import matplotlib; matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.patches import Rectangle
fig, ax = plt.subplots(figsize=(8, 9.6))
ax.add_patch(Rectangle((0, 0), W, H, fill=False, lw=2))
for r, b in boxes.items():
    ax.add_patch(Rectangle((b[0], b[1]), b[2] - b[0], b[3] - b[1], fill=True, alpha=.35, ec='k', lw=.5, fc='tab:red' if any(r in p for p in bad) else 'tab:blue'))
    ax.text((b[0] + b[2]) / 2, (b[1] + b[3]) / 2, r, ha='center', va='center', fontsize=4.5)
ax.set_xlim(-2, W + 2); ax.set_ylim(H + 5, -3); ax.set_aspect('equal')
fig.savefig('/tmp/minimeat-place/layout.png', dpi=140, bbox_inches='tight')
