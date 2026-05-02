use std::collections::HashMap;
use super::ast::{BinOp, Expr};
use super::units::{Dim, Quantity};

// ─────────────────────────────────────────────────────────────────────────────
// Full quantity evaluator — all arithmetic in SI
// ─────────────────────────────────────────────────────────────────────────────

pub fn eval_q(expr: &Expr, env: &HashMap<String, Quantity>) -> Result<Quantity, String> {
    match expr {
        Expr::Num(v) => Ok(Quantity::dimensionless(*v)),
        Expr::Var(n) => env.get(n).copied().ok_or_else(|| format!("undefined variable: '{n}'")),
        Expr::UnaryMinus(e) => {
            let q = eval_q(e, env)?;
            Ok(Quantity { val: -q.si_val(), dim: q.dim, scale: 1.0 })
        }
        Expr::BinOp(l, op, r) => {
            let lq = eval_q(l, env)?;
            let rq = eval_q(r, env)?;
            let ls = lq.si_val();
            let rs = rq.si_val();
            match op {
                BinOp::Add | BinOp::Sub => {
                    if lq.dim != rq.dim && !lq.dim.is_dimensionless() && !rq.dim.is_dimensionless() {
                        return Err(format!(
                            "dimension mismatch: cannot add {:?} and {:?}", lq.dim, rq.dim
                        ));
                    }
                    let v = if matches!(op, BinOp::Add) { ls + rs } else { ls - rs };
                    Ok(Quantity { val: v, dim: lq.dim, scale: 1.0 })
                }
                BinOp::Mul => Ok(Quantity { val: ls * rs, dim: lq.dim.mul(rq.dim), scale: 1.0 }),
                BinOp::Div => {
                    if rs == 0.0 { return Err("division by zero".into()); }
                    Ok(Quantity { val: ls / rs, dim: lq.dim.div(rq.dim), scale: 1.0 })
                }
            }
        }
        Expr::Pow(b, e) => {
            let bq = eval_q(b, env)?;
            let eq = eval_q(e, env)?;
            if !eq.dim.is_dimensionless() {
                return Err("exponent must be dimensionless".into());
            }
            let exp     = eq.si_val();
            let base_si = bq.si_val();
            let exp_int = exp.round() as i32;
            let dim = if (exp - exp_int as f64).abs() < 1e-9 {
                bq.dim.pow_int(exp_int)
            } else {
                if !bq.dim.is_dimensionless() {
                    return Err("fractional power of dimensioned quantity".into());
                }
                Dim::NONE
            };
            Ok(Quantity { val: base_si.powf(exp), dim, scale: 1.0 })
        }
        Expr::Call(name, args) => {
            match name.as_str() {
                "diff" => {
                    if args.len() != 2 { return Err("diff(expr, var) needs 2 args".into()); }
                    let var = match &args[1] {
                        Expr::Var(v) => v.clone(),
                        _ => return Err("diff: second arg must be variable name".into()),
                    };
                    let sym_d = symbolic_diff(&args[0], &var);
                    eval_q(&sym_d, env)
                }
                "integrate" => {
                    if args.len() != 4 { return Err("integrate(expr, var, a, b) needs 4 args".into()); }
                    let var = match &args[1] {
                        Expr::Var(v) => v.clone(),
                        _ => return Err("integrate: second arg must be variable name".into()),
                    };
                    let a = eval_q(&args[2], env)?.val;
                    let b = eval_q(&args[3], env)?.val;
                    Ok(Quantity::dimensionless(numeric_integrate(&args[0], &var, a, b, env)?))
                }
                "sum" => {
                    if args.len() != 4 { return Err("sum(expr, var, a, b) needs 4 args".into()); }
                    let var = match &args[1] {
                        Expr::Var(v) => v.clone(),
                        _ => return Err("sum: second arg must be variable name".into()),
                    };
                    let a = eval_q(&args[2], env)?.val.round() as i64;
                    let b = eval_q(&args[3], env)?.val.round() as i64;
                    Ok(Quantity::dimensionless(numeric_sum(&args[0], &var, a, b, env)?))
                }
                _ => {
                    if args.len() != 1 { return Err(format!("'{name}' expects 1 argument")); }
                    let q = eval_q(&args[0], env)?;
                    let v = q.si_val();
                    match name.as_str() {
                        "sqrt" => {
                            let mut d = [0i8; 7];
                            for i in 0..7 { d[i] = q.dim.0[i] / 2; }
                            Ok(Quantity { val: v.sqrt(), dim: Dim(d), scale: 1.0 })
                        }
                        "sin"  => Ok(Quantity::dimensionless(v.sin())),
                        "cos"  => Ok(Quantity::dimensionless(v.cos())),
                        "tan"  => Ok(Quantity::dimensionless(v.tan())),
                        "abs"  => Ok(Quantity { val: v.abs(),  dim: q.dim, scale: 1.0 }),
                        "ln"   => Ok(Quantity::dimensionless(v.ln())),
                        "log"  => Ok(Quantity::dimensionless(v.log10())),
                        "exp"  => Ok(Quantity::dimensionless(v.exp())),
                        "asin" => Ok(Quantity::dimensionless(v.asin())),
                        "acos" => Ok(Quantity::dimensionless(v.acos())),
                        "atan" => Ok(Quantity::dimensionless(v.atan())),
                        "ceil" => Ok(Quantity { val: v.ceil(),  dim: q.dim, scale: 1.0 }),
                        "floor"=> Ok(Quantity { val: v.floor(), dim: q.dim, scale: 1.0 }),
                        other  => Err(format!("unknown function: '{other}'")),
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Simple scalar eval (used by Typst codegen for substitution display)
// ─────────────────────────────────────────────────────────────────────────────

pub fn eval(expr: &Expr, env: &HashMap<String, f64>) -> Result<f64, String> {
    match expr {
        Expr::Num(v) => Ok(*v),
        Expr::Var(n) => env.get(n).copied().ok_or_else(|| format!("undefined: '{n}'")),
        Expr::UnaryMinus(e) => Ok(-eval(e, env)?),
        Expr::BinOp(l, op, r) => {
            let lv = eval(l, env)?;
            let rv = eval(r, env)?;
            Ok(match op {
                BinOp::Add => lv + rv,
                BinOp::Sub => lv - rv,
                BinOp::Mul => lv * rv,
                BinOp::Div => { if rv == 0.0 { return Err("division by zero".into()); } lv / rv }
            })
        }
        Expr::Pow(b, e) => Ok(eval(b, env)?.powf(eval(e, env)?)),
        Expr::Call(name, args) => {
            if args.len() != 1 { return Err(format!("'{name}' expects 1 arg in scalar eval")); }
            let v = eval(&args[0], env)?;
            match name.as_str() {
                "sqrt" => Ok(v.sqrt()), "sin"  => Ok(v.sin()),  "cos"  => Ok(v.cos()),
                "tan"  => Ok(v.tan()),  "abs"  => Ok(v.abs()),  "ln"   => Ok(v.ln()),
                "log"  => Ok(v.log10()),"exp"  => Ok(v.exp()),  "asin" => Ok(v.asin()),
                "acos" => Ok(v.acos()), "atan" => Ok(v.atan()), "ceil" => Ok(v.ceil()),
                "floor"=> Ok(v.floor()),
                other  => Err(format!("unknown function: '{other}'")),
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Symbolic differentiation
// ─────────────────────────────────────────────────────────────────────────────

pub fn symbolic_diff(expr: &Expr, var: &str) -> Expr {
    use BinOp::*;
    let zero  = || Expr::Num(0.0);
    let one   = || Expr::Num(1.0);
    let add   = |a: Expr, b: Expr| Expr::BinOp(Box::new(a), Add, Box::new(b));
    let sub   = |a: Expr, b: Expr| Expr::BinOp(Box::new(a), Sub, Box::new(b));
    let mul   = |a: Expr, b: Expr| Expr::BinOp(Box::new(a), Mul, Box::new(b));
    let div   = |a: Expr, b: Expr| Expr::BinOp(Box::new(a), Div, Box::new(b));
    let pow   = |a: Expr, b: Expr| Expr::Pow(Box::new(a), Box::new(b));
    let call1 = |n: &str, a: Expr| Expr::Call(n.to_string(), vec![a]);

    match expr {
        Expr::Num(_) => zero(),
        Expr::Var(v) => if v == var { one() } else { zero() },
        Expr::UnaryMinus(e) => Expr::UnaryMinus(Box::new(symbolic_diff(e, var))),
        Expr::BinOp(l, op, r) => {
            let dl = symbolic_diff(l, var);
            let dr = symbolic_diff(r, var);
            match op {
                Add => add(dl, dr),
                Sub => sub(dl, dr),
                Mul => add(mul(dl, *r.clone()), mul(*l.clone(), dr)),
                Div => div(
                    sub(mul(dl, *r.clone()), mul(*l.clone(), dr)),
                    pow(*r.clone(), Expr::Num(2.0)),
                ),
            }
        }
        Expr::Pow(base, exp) => {
            let db = symbolic_diff(base, var);
            let de = symbolic_diff(exp, var);
            let dep_base = contains_var(base, var);
            let dep_exp  = contains_var(exp, var);
            if dep_exp && !dep_base {
                mul(mul(pow(*base.clone(), *exp.clone()), call1("ln", *base.clone())), de)
            } else if dep_base && !dep_exp {
                mul(mul(*exp.clone(), pow(*base.clone(), sub(*exp.clone(), Expr::Num(1.0)))), db)
            } else {
                mul(
                    pow(*base.clone(), *exp.clone()),
                    add(mul(de, call1("ln", *base.clone())),
                        mul(*exp.clone(), div(db, *base.clone())))
                )
            }
        }
        Expr::Call(name, args) if args.len() == 1 => {
            let inner = &args[0];
            let di = symbolic_diff(inner, var);
            let outer_deriv = match name.as_str() {
                "sin"  => call1("cos", inner.clone()),
                "cos"  => Expr::UnaryMinus(Box::new(call1("sin", inner.clone()))),
                "tan"  => div(one(), pow(call1("cos", inner.clone()), Expr::Num(2.0))),
                "sqrt" => div(one(), mul(Expr::Num(2.0), call1("sqrt", inner.clone()))),
                "ln"   => div(one(), inner.clone()),
                "log"  => div(one(), mul(inner.clone(), call1("ln", Expr::Num(10.0_f64)))),
                "exp"  => call1("exp", inner.clone()),
                "asin" => div(one(), call1("sqrt", sub(Expr::Num(1.0), pow(inner.clone(), Expr::Num(2.0))))),
                "acos" => Expr::UnaryMinus(Box::new(
                    div(one(), call1("sqrt", sub(Expr::Num(1.0), pow(inner.clone(), Expr::Num(2.0))))))),
                "atan" => div(one(), add(Expr::Num(1.0), pow(inner.clone(), Expr::Num(2.0)))),
                "abs"  => div(inner.clone(), call1("abs", inner.clone())),
                _      => return Expr::Num(f64::NAN),
            };
            mul(outer_deriv, di)
        }
        _ => Expr::Num(f64::NAN),
    }
}

fn contains_var(expr: &Expr, var: &str) -> bool {
    match expr {
        Expr::Var(v) => v == var,
        Expr::Num(_) => false,
        Expr::UnaryMinus(e) => contains_var(e, var),
        Expr::BinOp(l, _, r) => contains_var(l, var) || contains_var(r, var),
        Expr::Pow(b, e) => contains_var(b, var) || contains_var(e, var),
        Expr::Call(_, args) => args.iter().any(|a| contains_var(a, var)),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Numeric integration (Gauss-Legendre 16-point)
// ─────────────────────────────────────────────────────────────────────────────

fn numeric_integrate(expr: &Expr, var: &str, a: f64, b: f64, env: &HashMap<String, Quantity>) -> Result<f64, String> {
    const NODES: [f64; 16] = [
        -0.9894009349916499, -0.9445750230732326, -0.8656312023341258, -0.7554044083550030,
        -0.6178762444026438, -0.4580167776572274, -0.2816035507792589, -0.0950125098360223,
         0.0950125098360223,  0.2816035507792589,  0.4580167776572274,  0.6178762444026438,
         0.7554044083550030,  0.8656312023341258,  0.9445750230732326,  0.9894009349916499,
    ];
    const WEIGHTS: [f64; 16] = [
        0.0271524594117541, 0.0622535239386479, 0.0951585116824928, 0.1246289712555339,
        0.1495959888165767, 0.1691565193950025, 0.1826034150449236, 0.1894506104550685,
        0.1894506104550685, 0.1826034150449236, 0.1691565193950025, 0.1495959888165767,
        0.1246289712555339, 0.0951585116824928, 0.0622535239386479, 0.0271524594117541,
    ];
    let mid  = (a + b) / 2.0;
    let half = (b - a) / 2.0;
    let mut sum = 0.0;
    for (&t, &w) in NODES.iter().zip(WEIGHTS.iter()) {
        let x = mid + half * t;
        let mut local_env = env.clone();
        local_env.insert(var.to_string(), Quantity::dimensionless(x));
        sum += w * eval_q(expr, &local_env)?.val;
    }
    Ok(half * sum)
}

// ─────────────────────────────────────────────────────────────────────────────
// Numeric summation
// ─────────────────────────────────────────────────────────────────────────────

fn numeric_sum(expr: &Expr, var: &str, a: i64, b: i64, env: &HashMap<String, Quantity>) -> Result<f64, String> {
    if b - a > 100_000 { return Err(format!("sum range too large ({} terms)", b - a + 1)); }
    let mut total = 0.0;
    for k in a..=b {
        let mut local_env = env.clone();
        local_env.insert(var.to_string(), Quantity::dimensionless(k as f64));
        total += eval_q(expr, &local_env)?.val;
    }
    Ok(total)
}
