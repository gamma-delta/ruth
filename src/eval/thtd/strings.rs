//! Messing with strings.

use super::*;

pub fn to_string(engine: &mut Engine, _: Gc<GcCell<Namespace>>, args: &[Gc<Expr>]) -> EvalResult {
    let mut out = String::new();
    for expr in args {
        out.push_str(&engine.print_expr(expr.to_owned())?);
    }
    Ok(Gc::new(Expr::String(out)))
}

pub fn string_len(engine: &mut Engine, _: Gc<GcCell<Namespace>>, args: &[Gc<Expr>]) -> EvalResult {
    check_argc(engine, args, 1, 1)?;

    if let Expr::String(s) = &*args[0] {
        Ok(Gc::new(Expr::Integer(s.len() as i64)))
    } else {
        Err(bad_arg_type(engine, args[0].to_owned(), 0, "string"))
    }
}

pub fn string_slice(
    engine: &mut Engine,
    _: Gc<GcCell<Namespace>>,
    args: &[Gc<Expr>],
) -> EvalResult {
    check_argc(engine, args, 2, 3)?;

    let string = match &*args[0] {
        Expr::String(s) => s.as_str(),
        _ => return Err(bad_arg_type(engine, args[0].to_owned(), 0, "string")),
    };

    let start = args[1].to_owned();
    let start = match &*start {
        Expr::Integer(i) if *i >= 0 => *i as usize,
        _ if !engine.is_truthy(start.to_owned()) => 0,
        _ => return Err(bad_arg_type(engine, start, 1, "positive int or falsy")),
    };
    let end = args[2].to_owned();
    let end = match &*end {
        Expr::Integer(i) if *i >= 0 => *i as usize,
        _ if !engine.is_truthy(end.to_owned()) => string.len(),
        _ => return Err(bad_arg_type(engine, end, 2, "positive int or falsy")),
    };

    if !string.is_char_boundary(start) {
        return Err(engine.make_err(
            "string/slice-boundary",
            format!("{} is not on a char boundary", start),
            None,
        ));
    }
    if !string.is_char_boundary(end) {
        return Err(engine.make_err(
            "string/slice-boundary",
            format!("{} is not on a char boundary", end),
            None,
        ));
    }

    if start > end {
        return Err(engine.make_err(
            "string/slice-out-of-order",
            format!("the start {} was after the end {}", start, end),
            Some(Engine::list_to_sexp(&[
                Gc::new(Expr::Integer(start as _)),
                Gc::new(Expr::Integer(end as _)),
            ])),
        ));
    }

    let too_far = if start >= string.len() {
        Some(start)
    } else if end >= string.len() {
        Some(end)
    } else {
        None
    };
    if let Some(too_far) = too_far {
        return Err(engine.make_err(
            "string/slice-too-far",
            format!(
                "{} was out of bounds (string had len {})",
                too_far,
                string.len()
            ),
            Some(Engine::list_to_sexp(&[
                Gc::new(Expr::Integer(too_far as _)),
                Gc::new(Expr::Integer(string.len() as _)),
            ])),
        ));
    }

    // finally
    let slice = string.get(start..end);
    if let Some(slice) = slice {
        Ok(Gc::new(Expr::String(slice.to_owned())))
    } else {
        Err(engine.make_err(
            "string/slice-unknown",
            format!(
                "some other error happened when slicing {:?} at {}..{}",
                &string, start, end
            ),
            Some(Engine::list_to_sexp(&[
                args[0].to_owned(),
                Gc::new(Expr::Integer(start as _)),
                Gc::new(Expr::Integer(end as _)),
            ])),
        ))
    }
}

pub fn string_find(engine: &mut Engine, _: Gc<GcCell<Namespace>>, args: &[Gc<Expr>]) -> EvalResult {
    let needle = match &*args[0] {
        Expr::String(s) => s,
        _ => return Err(bad_arg_type(engine, args[0].to_owned(), 0, "string")),
    };
    let haystack = match &*args[1] {
        Expr::String(s) => s,
        _ => return Err(bad_arg_type(engine, args[1].to_owned(), 1, "string")),
    };
    Ok(match haystack.find(needle) {
        Some(idx) => Gc::new(Expr::Integer(idx as _)),
        None => engine.make_bool(false),
    })
}

pub fn prn(engine: &mut Engine, _: Gc<GcCell<Namespace>>, args: &[Gc<Expr>]) -> EvalResult {
    check_argc(engine, args, 1, 2)?;

    let newline = if let Some(check) = args.get(1) {
        engine.is_truthy(check.to_owned())
    } else {
        true
    };

    let out = engine.print_expr(args[0].clone())?;

    if newline {
        println!("{}", out);
    } else {
        print!("{}", out);
    }
    std::io::stdout().flush().unwrap();

    Ok(args[0].to_owned())
}
