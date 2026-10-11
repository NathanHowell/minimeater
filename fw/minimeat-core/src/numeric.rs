//! Small numeric helpers shared by the models (no_std, no alloc).

/// Brent's method on a bracketing interval [a, b] with f(a), f(b) of opposite sign (or zero).
/// Same algorithm as scipy's `brentq`; `None` if the interval does not bracket a root.
pub fn brent(mut f: impl FnMut(f64) -> f64, mut a: f64, mut b: f64, xtol: f64) -> Option<f64> {
    let (mut fa, mut fb) = (f(a), f(b));
    if fa == 0.0 {
        return Some(a);
    }
    if fb == 0.0 {
        return Some(b);
    }
    if fa.signum() == fb.signum() {
        return None;
    }
    let (mut c, mut fc) = (a, fa);
    let (mut d, mut e) = (b - a, b - a);
    for _ in 0..200 {
        if fb.signum() == fc.signum() {
            c = a;
            fc = fa;
            d = b - a;
            e = d;
        }
        if fc.abs() < fb.abs() {
            a = b;
            b = c;
            c = a;
            fa = fb;
            fb = fc;
            fc = fa;
        }
        let tol = 2.0 * f64::EPSILON * b.abs() + 0.5 * xtol;
        let m = 0.5 * (c - b);
        if m.abs() <= tol || fb == 0.0 {
            return Some(b);
        }
        if e.abs() >= tol && fa.abs() > fb.abs() {
            let s = fb / fa;
            let (mut p, mut q) = if a == c {
                (2.0 * m * s, 1.0 - s)
            } else {
                let (q, r) = (fa / fc, fb / fc);
                (
                    s * (2.0 * m * q * (q - r) - (b - a) * (r - 1.0)),
                    (q - 1.0) * (r - 1.0) * (s - 1.0),
                )
            };
            if p > 0.0 {
                q = -q;
            } else {
                p = -p;
            }
            if 2.0 * p < (3.0 * m * q - (tol * q).abs()).min((e * q).abs()) {
                e = d;
                d = p / q;
            } else {
                d = m;
                e = m;
            }
        } else {
            d = m;
            e = m;
        }
        a = b;
        fa = fb;
        b += if d.abs() > tol { d } else { tol.copysign(m) };
        fb = f(b);
    }
    Some(b)
}

/// Linear interpolation in a non-decreasing table, clamped at the ends (`np.interp`).
pub fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let n = xs.len();
    if x <= xs[0] {
        return ys[0];
    }
    if x >= xs[n - 1] {
        return ys[n - 1];
    }
    let j = xs.partition_point(|&v| v <= x).clamp(1, n - 1);
    let (x0, x1) = (xs[j - 1], xs[j]);
    if x1 == x0 {
        ys[j]
    } else {
        ys[j - 1] + (x - x0) / (x1 - x0) * (ys[j] - ys[j - 1])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brent_finds_roots() {
        let r = brent(|x| x * x - 2.0, 0.0, 2.0, 1e-14).unwrap();
        assert!((r - libm::sqrt(2.0)).abs() < 1e-12);
        assert!(brent(|x| x * x + 1.0, 0.0, 2.0, 1e-12).is_none());
    }

    #[test]
    fn interp_clamps_and_lerps() {
        let (xs, ys) = ([0.0, 1.0, 3.0], [10.0, 20.0, 40.0]);
        assert_eq!(interp(-1.0, &xs, &ys), 10.0);
        assert_eq!(interp(2.0, &xs, &ys), 30.0);
        assert_eq!(interp(9.0, &xs, &ys), 40.0);
    }
}
