//! Integer recovery of substitution coefficients (“lambdas”).
//!
//! Given the observed signatures of an expression and one or two candidate
//! zero-signature expressions, these routines recover the integer multipliers
//! that turn the expression into a target constant signature, working over the
//! signed interpretation of the active bit width.

use crate::varint::make_mask;

use log::debug;

fn get_signed(x: u64, n: u8) -> i64 {
    let value = x & make_mask(n);
    let shift = 64 - n;
    ((value << shift) as i64) >> shift // arithmetic shift
}

pub(super) fn find_lambda_int(x: &[u64], y: &[u64], a: i64, b: i64, n: u8) -> Option<u64> {
    let mut valid: Option<(Option<i64>, Option<i64>)> = None;

    for (&xi, &yi) in x.iter().zip(y.iter()) {
        let xi = get_signed(xi, n);
        let yi = get_signed(yi, n);

        if yi == 0 {
            if xi == a || xi == b {
                continue;
            } else {
                return None;
            }
        }

        let mut vals = [None, None];

        if (xi.wrapping_sub(a)) % yi == 0 {
            vals[0] = Some((xi.wrapping_sub(a)) / yi);
        }

        if (xi.wrapping_sub(b)) % yi == 0 {
            vals[1] = Some((xi.wrapping_sub(b)) / yi);
        }

        match valid {
            None => valid = Some((vals[0], vals[1])),
            Some((va, vb)) => {
                let mut new = (None, None);
                for v in vals.iter().flatten() {
                    if va == Some(*v) || vb == Some(*v) {
                        if new.0.is_none() {
                            new.0 = Some(*v);
                        } else {
                            new.1 = Some(*v);
                        }
                    }
                }
                valid = Some(new);
            }
        }

        if let Some((None, None)) = valid {
            debug!("OVERCONSTRAINED");
            return None;
        }
    }

    if let Some(valid) = valid {
        let mask = make_mask(n);

        if let Some(v) = valid.0
            && get_signed(x[0], n).wrapping_sub(v.wrapping_mul(get_signed(y[0], n))) == a
        {
            return Some((v as u64) & mask);
        }

        if let Some(v) = valid.1
            && get_signed(x[0], n).wrapping_sub(v.wrapping_mul(get_signed(y[0], n))) == a
        {
            return Some((v as u64) & mask);
        }

        None
    } else {
        // the vector is null
        Some(0)
    }
}

pub(super) fn find_two_lambdas_int(
    x: &[u64],
    y: &[u64],
    z: &[u64],
    a: i64,
    b: i64,
    n: u8,
) -> Option<(u64, u64)> {
    let signed: Vec<_> = x
        .iter()
        .zip(y)
        .zip(z)
        .map(|((&x, &y), &z)| (get_signed(x, n), get_signed(y, n), get_signed(z, n)))
        .collect();
    let base = signed.iter().position(|&(_, y, z)| y != 0 || z != 0)?;
    let targets = [a, b];

    for second in 0..signed.len() {
        let (_, y0, z0) = signed[base];
        let (_, y1, z1) = signed[second];
        let determinant = y0 as i128 * z1 as i128 - y1 as i128 * z0 as i128;
        if determinant == 0 {
            continue;
        }

        for target0 in targets {
            for target1 in targets {
                let rhs0 = signed[base].0.wrapping_sub(target0) as i128;
                let rhs1 = signed[second].0.wrapping_sub(target1) as i128;
                let lambda_y_num = rhs0 * z1 as i128 - rhs1 * z0 as i128;
                let lambda_z_num = y0 as i128 * rhs1 - y1 as i128 * rhs0;
                if lambda_y_num % determinant != 0 || lambda_z_num % determinant != 0 {
                    continue;
                }

                let lambda_y = lambda_y_num / determinant;
                let lambda_z = lambda_z_num / determinant;
                let Ok(lambda_y) = i64::try_from(lambda_y) else {
                    continue;
                };
                let Ok(lambda_z) = i64::try_from(lambda_z) else {
                    continue;
                };

                let corrected = |(x, y, z): (i64, i64, i64)| {
                    x.wrapping_sub(lambda_y.wrapping_mul(y))
                        .wrapping_sub(lambda_z.wrapping_mul(z))
                };
                if corrected(signed[0]) == a
                    && signed.iter().copied().all(|values| {
                        let value = corrected(values);
                        value == a || value == b
                    })
                {
                    let mask = make_mask(n);
                    return Some(((lambda_y as u64) & mask, (lambda_z as u64) & mask));
                }
            }
        }
    }

    None
}
