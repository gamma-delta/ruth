use std::collections::HashMap;

use gc::Gc;

use crate::{Engine, Exception, Expr, Symbol};

/// Why this doesn't exist in the stdlib I'll never know.
macro_rules! hashmap {
    ($($key:expr => $val:expr),* $(,)*) => {{
        #[allow(unused_mut)]
        let mut map = HashMap::new();
        $(
            map.insert($key, $val);
        )*
        map
    }};
}

impl Engine {
    /// Given an argument spec and values to fill it, return a shiny new namespace for the arguments.
    pub fn destructure_assign(
        &mut self,
        spec: Gc<Expr>,
        val: Gc<Expr>,
    ) -> Result<HashMap<Symbol, Gc<Expr>>, Exception> {
        fn recurse_opt(
            engine: &mut Engine,
            spec: Gc<Expr>,
            val: Option<Gc<Expr>>,
        ) -> Result<HashMap<Symbol, Gc<Expr>>, Exception> {
            let default_sym = engine.intern_symbol("default");

            if let Some(val) = val {
                recurse(engine, spec, val)
            } else {
                Err(engine.make_err(
                    "assignment/no-default",
                    "lack of value with no default".to_owned(),
                    None,
                ))
            }
        }

        fn recurse(
            engine: &mut Engine,
            spec: Gc<Expr>,
            val: Gc<Expr>,
        ) -> Result<HashMap<Symbol, Gc<Expr>>, Exception> {
            let underscore = engine.intern_symbol("_");

            println!(
                "{} <- {}",
                engine.write_expr(spec.to_owned()).unwrap(),
                engine.write_expr(val.to_owned()).unwrap()
            );

            match (&*spec, &*val) {
                // ignore _
                (Expr::Symbol(sym), _) if *sym == underscore => Ok(HashMap::new()),
                // plain ol variable binding
                (Expr::Symbol(sym), _) => Ok(hashmap! {*sym => val}),

                (Expr::Pair(spec_car, spec_cdr), Expr::Pair(val_car, val_cdr)) => {
                    let mut out = HashMap::new();

                    out.extend(recurse(engine, spec_car.to_owned(), val_car.to_owned())?);
                    out.extend(recurse(engine, spec_cdr.to_owned(), val_cdr.to_owned())?);

                    Ok(out)
                }
                // For the case when we have (x y z) matching with (1 2)
                // We try to match (y . (z . ())) with (2 . ()) -> (z . ()) with nil
                // so when we have (cons z ...) with nil, z gets no value.
                // when we match a pair with nil, that's the sign for no more values for the specs.
                // (Then it gets "stuck" inside the None case.)
                (Expr::Pair(spec_car, spec_cdr), Expr::Nil) => {
                    let mut out = HashMap::new();

                    out.extend(recurse_opt(engine, spec_car.to_owned(), None)?);
                    out.extend(recurse_opt(engine, spec_cdr.to_owned(), None)?);

                    Ok(out)
                }
                (Expr::Map(spec_map), Expr::Map(val_map)) => {
                    let mut out = HashMap::with_capacity(spec_map.capacity());

                    for (spec_k, spec_v) in spec_map.iter() {
                        if let Some(val_v) = val_map.get(spec_k) {
                            out.extend(recurse(engine, spec_v.to_owned(), val_v.to_owned())?);
                        } else {
                            out.extend(recurse_opt(engine, spec_k.to_owned(), None)?);
                        }
                    }

                    Ok(out)
                }

                _ if spec == val => {
                    // Well, it matches ... just return an empty namespace
                    Ok(HashMap::new())
                }
                _ => {
                    let s = engine.write_expr(spec.to_owned())?;
                    let v = engine.write_expr(val.to_owned())?;
                    Err(engine.make_err(
                        "assignment/invalid",
                        format!("cannot bind {} to {}", v, s),
                        Some(Engine::list_to_sexp(&[spec, val])),
                    ))
                }
            }
        }

        // None -> this was not a spec with default
        // Some -> it was and here's the deets
        fn check_default(
            engine: &mut Engine,
            maybe_default: Gc<Expr>,
            val: Option<Gc<Expr>>,
        ) -> Option<Result<HashMap<Symbol, Gc<Expr>>, Exception>> {
            // (default spec otherwise) tries to match to the spec, and if not fills it with the default and tries again with the next.
            // this is some Good Code :tm:
            if let Expr::Pair(car, cdr) = &*maybe_default {
                if let Some(list) = engine.sexp_to_list(car.to_owned())? {
                    match list.as_slice() {
                        [default_checker, ..] if **default_checker == Expr::Symbol(default_sym) => {
                            // ok we have something starting with `default` let's give it a try
                            if let [_, spec, default] = list.as_slice() {
                                // If we have any value at all try that
                                if let Some(val) = &val {
                                    // does it match?
                                    if let Ok(bindings) =
                                        recurse(engine, spec.to_owned(), val.to_owned())
                                    {
                                        // it does!
                                        return Ok(bindings);
                                    }
                                }
                                // Something went wrong; try to use the default argument...
                                let mut defaulted =
                                    recurse(engine, spec.to_owned(), default.to_owned())?;
                                // ... and pass the troublemaker value off to the next spec
                                defaulted.extend(recurse_opt(engine, cdr.to_owned(), val)?);
                                return Ok(defaulted);
                            }
                        }
                        _ => {}
                    }
                }
            }

            None
        }

        recurse(self, spec, val)
    }
}