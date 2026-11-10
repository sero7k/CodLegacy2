#!/usr/bin/env python3
"""compare.py <shots dir> <out dir>: HMW reference vs our iv_<map>.png per map.
Writes per-map pairs, 3-per-sheet contact sheets, and scores.csv."""
import os, sys, csv
import numpy as np
from PIL import Image, ImageDraw, ImageFont
REFS = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'docs', 'look_refs')
shots, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
font = ImageFont.truetype('/usr/share/fonts/truetype/dejavu/DejaVuSans-Bold.ttf', 20)
def ref_img(m):
    f = os.path.join(REFS, m, 'hmw_intermission.jpg')
    im = Image.open(f).convert('RGB'); w, h = im.size
    # desktop capture: window title bar on top, taskbar at the bottom
    return im.crop((0, int(h * 0.028), w, int(h * 0.945)))
def stats(im):
    a = np.asarray(im.resize((320, 180))).astype(float) / 255
    l = (a * [0.2126, 0.7152, 0.0722]).sum(2)
    p = np.percentile(l, [10, 50, 90])
    rgb = a.reshape(-1, 3).mean(0)
    return dict(p10=p[0], p50=p[1], p90=p[2], r=rgb[0], g=rgb[1], b=rgb[2],
                sat=float((a.max(2) - a.min(2)).mean()))
rows, pairs = [], []
for fn in sorted(os.listdir(shots)):
    if not (fn.startswith('iv_') and fn.endswith('.png')):
        continue
    tag = fn[3:-4]; m = tag.split('__')[0]; ours = os.path.join(shots, fn)
    if not os.path.isdir(os.path.join(REFS, m)):
        continue
    r, o = ref_img(m).resize((640, 360)), Image.open(ours).convert('RGB').resize((640, 360))
    sr, so = stats(r), stats(o)
    row = {'map': tag}
    for k in sr:
        row['ref_' + k] = round(sr[k], 3); row['our_' + k] = round(so[k], 3)
    row['d_p50'] = round(so['p50'] - sr['p50'], 3); row['d_p10'] = round(so['p10'] - sr['p10'], 3)
    row['d_sat'] = round(so['sat'] - sr['sat'], 3)
    rows.append(row); pairs.append((tag, r, o, row))
with open(os.path.join(out, 'scores.csv'), 'w', newline='') as f:
    w = csv.DictWriter(f, fieldnames=list(rows[0].keys())); w.writeheader(); w.writerows(rows)
for i in range(0, len(pairs), 3):
    ch = pairs[i:i + 3]
    sheet = Image.new('RGB', (1280, 392 * len(ch)), (18, 18, 18)); d = ImageDraw.Draw(sheet)
    for j, (m, r, o, row) in enumerate(ch):
        y = j * 392
        sheet.paste(r, (0, y + 32)); sheet.paste(o, (640, y + 32))
        d.text((6, y + 6), f'mp_{m} - HMW', fill=(255, 220, 120), font=font)
        d.text((646, y + 6), f'mp_{m} - OURS  (mid {row["our_p50"]:.2f} vs {row["ref_p50"]:.2f}, dark {row["our_p10"]:.2f} vs {row["ref_p10"]:.2f})', fill=(120, 220, 255), font=font)
    sheet.save(os.path.join(out, f'cmp_{i // 3 + 1:02d}.jpg'), quality=80)
print(len(rows), 'maps compared')
import statistics as st
for k in ['d_p50', 'd_p10', 'd_sat']:
    v = [r[k] for r in rows]; print(k, 'median', round(st.median(v), 3), 'min', min(v), 'max', max(v))
