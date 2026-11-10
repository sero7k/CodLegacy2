#!/usr/bin/env python3
"""Line up our screenshot with a real-game reference of the same spot and
compare them: per-region brightness and colour, and a side-by-side picture.

    compare.py ours.png reference.png out_prefix

Our shot's HUD and gun (the top bar, minimap, bottom third's middle) are
masked. Alignment: ORB features and a RANSAC homography from ours onto the
reference; the reference's sky (the top band) is compared separately.
"""
import sys, json
import cv2
import numpy as np

def load(p):
    im = cv2.imread(p, cv2.IMREAD_COLOR)
    if im is None:
        sys.exit(f"cannot read {p}")
    return im

def mask_ours(h, w):
    m = np.full((h, w), 255, np.uint8)
    m[: int(h * 0.09), :] = 0                      # top bar
    m[: int(h * 0.27), : int(w * 0.15)] = 0        # minimap
    m[int(h * 0.70):, int(w * 0.45): int(w * 0.92)] = 0  # gun
    m[int(h * 0.75):, int(w * 0.94):] = 0          # weapon icons
    m[int(h * 0.94):, : int(w * 0.15)] = 0         # bottom left
    return m

def srgb_to_lin(x):
    x = x / 255.0
    return np.where(x <= 0.04045, x / 12.92, ((x + 0.055) / 1.055) ** 2.4)

def main():
    ours_p, ref_p, out = sys.argv[1:4]
    ours, ref = load(ours_p), load(ref_p)
    # Work at the reference's width.
    s = ref.shape[1] / ours.shape[1]
    ours = cv2.resize(ours, (ref.shape[1], int(round(ours.shape[0] * s))), interpolation=cv2.INTER_AREA)
    h, w = ours.shape[:2]
    m = mask_ours(h, w)
    g1 = cv2.cvtColor(ours, cv2.COLOR_BGR2GRAY)
    g2 = cv2.cvtColor(ref, cv2.COLOR_BGR2GRAY)
    # (Equalised for matching: the two differ in exposure and grade.)
    e1, e2 = cv2.equalizeHist(g1), cv2.equalizeHist(g2)
    # SIFT on gradient magnitude (lighting-robust), Lowe's ratio test, a
    # similarity (the cameras differ only in field of view and a little
    # position) by RANSAC.
    def grad(g):
        gx = cv2.Sobel(g, cv2.CV_32F, 1, 0); gy = cv2.Sobel(g, cv2.CV_32F, 0, 1)
        mag = cv2.magnitude(gx, gy)
        return cv2.normalize(mag, None, 0, 255, cv2.NORM_MINMAX).astype(np.uint8)
    sift = cv2.SIFT_create(8000)
    best = (None, 0)
    for a, b in ((e1, e2), (grad(g1), grad(g2))):
        k1, d1 = sift.detectAndCompute(a, m)
        k2, d2 = sift.detectAndCompute(b, None)
        if d1 is None or d2 is None:
            continue
        pairs = cv2.BFMatcher().knnMatch(d1, d2, k=2)
        good = [p[0] for p in pairs if len(p) == 2 and p[0].distance < 0.8 * p[1].distance]
        if len(good) < 10:
            continue
        src = np.float32([k1[x.queryIdx].pt for x in good]).reshape(-1, 1, 2)
        dst = np.float32([k2[x.trainIdx].pt for x in good]).reshape(-1, 1, 2)
        A, inl = cv2.estimateAffinePartial2D(src, dst, method=cv2.RANSAC, ransacReprojThreshold=6.0)
        n = int(inl.sum()) if inl is not None else 0
        if A is not None and n > best[1]:
            best = (np.vstack([A, [0, 0, 1]]), n)
    H, inliers = best
    if H is None or inliers < 20:
        print(json.dumps({"aligned": False, "inliers": inliers}))
        H = np.eye(3)
    warped = cv2.warpPerspective(ours, H, (ref.shape[1], ref.shape[0]))
    wm = cv2.warpPerspective(m, H, (ref.shape[1], ref.shape[0]))
    valid = wm > 128
    # Regions by the reference's own brightness (sky band, sunlit, shade).
    lr = srgb_to_lin(ref.astype(np.float32))[..., ::-1]
    lo = srgb_to_lin(warped.astype(np.float32))[..., ::-1]
    lum = lambda x: x[..., 0] * 0.2126 + x[..., 1] * 0.7152 + x[..., 2] * 0.0722
    Lr, Lo = lum(lr), lum(lo)
    yy = np.arange(ref.shape[0])[:, None].repeat(ref.shape[1], 1)
    sky = valid & (yy < ref.shape[0] * 0.25) & (Lr > np.percentile(Lr, 70))
    ground = valid & (yy > ref.shape[0] * 0.55)
    q = np.percentile(Lr[ground], [30, 80]) if ground.any() else [0, 1]
    shade = ground & (Lr < q[0])
    sun = ground & (Lr > q[1])
    def stats(sel):
        if sel.sum() < 50:
            return None
        return {
            "ref_lum": float(Lr[sel].mean()), "our_lum": float(Lo[sel].mean()),
            "ratio": float(Lo[sel].mean() / max(Lr[sel].mean(), 1e-6)),
            "ref_rgb": [float(v) for v in lr[sel].mean(0)], "our_rgb": [float(v) for v in lo[sel].mean(0)],
            "pixels": int(sel.sum()),
        }
    report = {"aligned": inliers >= 20, "inliers": inliers, "sky": stats(sky), "sunlit_ground": stats(sun), "shaded_ground": stats(shade), "all": stats(valid)}
    print(json.dumps(report, indent=1))
    # Side by side: ours aligned | reference | per-pixel ratio map.
    ratio = np.clip(Lo / np.maximum(Lr, 1e-3), 0.25, 4.0)
    rm = (np.log2(ratio) / 2.0 * 0.5 + 0.5)
    rm = cv2.applyColorMap((rm * 255).astype(np.uint8), cv2.COLORMAP_COOLWARM if hasattr(cv2, "COLORMAP_COOLWARM") else cv2.COLORMAP_JET)
    rm[~valid] = 0
    side = np.hstack([warped, ref, rm])
    cv2.imwrite(out + "_side.jpg", cv2.resize(side, (side.shape[1] // 2, side.shape[0] // 2)), [cv2.IMWRITE_JPEG_QUALITY, 85])
    json.dump(report, open(out + ".json", "w"), indent=1)

if __name__ == "__main__":
    main()
