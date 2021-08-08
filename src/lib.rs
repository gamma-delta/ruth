mod eval;
mod parse;

use eval::TailRec;
pub use parse::{ExprParseError, ExprParseErrorInfo};

use std::{
    collections::HashMap,
    fmt::{self, Write},
};

#[macro_use]
extern crate gc;
#[macro_use]
extern crate derivative;

use bimap::BiHashMap;
use gc::{Finalize, Gc, GcCell, Trace};

type Symbol = u64;

#[derive(Derivative, Trace, Finalize)]
#[derivative(Debug)]
pub enum Expr {
    Integer(i64),
    Float(f64),
    String(String),
    /// Interned string.
    ///
    /// This number is the ID of this symbol and is used for quick equality
    /// and looking up the original string.
    Symbol(Symbol),
    /// Pointer to two elements.
    Pair(Gc<Expr>, Gc<Expr>),
    /// Lack of a value
    Nil,

    /// Named native special "function" like define, and the symbol of its name.
    SpecialForm {
        #[derivative(Debug(format_with = "Expr::form_formatter"))]
        #[unsafe_ignore_trace]
        func: fn(&mut Engine, Gc<GcCell<Namespace>>, &[Gc<Expr>]) -> TailRec,
        name: Symbol,
    },
    /// Named native function and the symbol of its name.
    NativeProcedure {
        #[derivative(Debug(format_with = "Expr::func_formatter"))]
        #[unsafe_ignore_trace]
        func: fn(&mut Engine, Gc<GcCell<Namespace>>, &[Gc<Expr>]) -> Gc<Expr>,
        name: Symbol,
    },

    Procedure {
        args: Vec<(Symbol, Option<Gc<Expr>>)>,
        body: Vec<Gc<Expr>>,
        env: Gc<GcCell<Namespace>>,
        variadic: bool,
        /// If false this is a macro
        is_lambda: bool,
    },
}

impl Expr {
    #[allow(clippy::type_complexity)]
    fn form_formatter(
        _: &fn(&mut Engine, Gc<GcCell<Namespace>>, &[Gc<Expr>]) -> TailRec,
        f: &mut std::fmt::Formatter,
    ) -> Result<(), std::fmt::Error> {
        write!(f, "fn(...)")
    }
    #[allow(clippy::type_complexity)]
    fn func_formatter(
        _: &fn(&mut Engine, Gc<GcCell<Namespace>>, &[Gc<Expr>]) -> Gc<Expr>,
        f: &mut std::fmt::Formatter,
    ) -> Result<(), std::fmt::Error> {
        write!(f, "fn(...)")
    }
}

/// Execution state and reader.
#[derive(Debug, Clone)]
pub struct Engine {
    /// Map of all known interned symbols to their handles, and vice versa
    interned_symbols: BiHashMap<String, Symbol>,
    /// Number of symbols that have ever been created
    akashic_symbol_count: u64,

    /// Standard library, aka top level namespace
    thtdlib: Gc<GcCell<Namespace>>,
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

impl Engine {
    pub fn new() -> Self {
        let mut out = Self {
            interned_symbols: BiHashMap::new(),
            akashic_symbol_count: 0,
            thtdlib: Gc::new(GcCell::new(Namespace {
                mappings: HashMap::new(),
                parent: None,
            })),
        };
        eval::add_thtandard_library(&mut out);
        out
    }

    /// Reads the source and return one token from it.
    pub fn read_one(&mut self, s: &str, source_name: String) -> Result<Expr, ExprParseError> {
        parse::read_one(s, source_name, self)
    }

    /// Reads the source and returns everything found in it.
    pub fn read_many(&mut self, s: &str, source_name: String) -> Result<Vec<Expr>, ExprParseError> {
        parse::read_many(s, source_name, self)
    }

    /// Read and eval everything in the source file, returning
    /// the item in tail position (or `()` if there isn't anything).
    pub fn read_eval(&mut self, s: &str, source_name: String) -> Result<Gc<Expr>, ExprParseError> {
        Ok(parse::read_many(s, source_name, self)?
            .into_iter()
            .map(|e| self.eval(self.thtdlib(), Gc::new(e)))
            .last()
            .unwrap_or_else(|| Gc::new(Expr::Nil)))
    }

    /// Write an expression to a string. Reading this string
    /// should yield the same as the original (with some caveats for procedures &c).
    pub fn write_expr(&self, expr: Gc<Expr>) -> String {
        fn recur(engine: &Engine, w: &mut impl Write, expr: Gc<Expr>) -> Result<(), fmt::Error> {
            match &*expr {
                Expr::Integer(i) => write!(w, "{}", i),
                Expr::Float(f) => write!(w, "{}", f),
                Expr::Symbol(sym) => {
                    if let Some(s) = engine.get_symbol_str(*sym) {
                        write!(w, "{}", s)
                    } else {
                        write!(w, "<unknown #{}>", sym)
                    }
                }
                Expr::Pair(car, cdr) => {
                    fn write_list<W: Write>(
                        engine: &Engine,
                        w: &mut W,
                        car: Gc<Expr>,
                        cdr: Gc<Expr>,
                    ) -> Result<(), fmt::Error> {
                        recur(engine, w, car)?;
                        match &*cdr {
                            // Proper list's end, do nothing
                            Expr::Nil => Ok(()),
                            // Proper list, leave space for the next thing.
                            Expr::Pair(cdar, cddr) => {
                                write!(w, " ")?;
                                write_list(engine, w, cdar.to_owned(), cddr.to_owned())
                            }
                            // Just a pair
                            _ => {
                                write!(w, " . ")?;
                                recur(engine, w, cdr.to_owned())
                            }
                        }
                    }

                    write!(w, "(")?;
                    write_list(engine, w, car.clone(), cdr.clone())?;
                    write!(w, ")")
                }
                Expr::Nil => {
                    write!(w, "()")
                }
                Expr::String(s) => {
                    write!(w, "{:?}", s)
                }
                Expr::SpecialForm { name, .. } => {
                    if let Some(name) = engine.get_symbol_str(*name) {
                        write!(w, "<special form {}>", name)
                    } else {
                        write!(w, "<anonymous special form>")
                    }
                }
                Expr::NativeProcedure { name, .. } => {
                    if let Some(name) = engine.get_symbol_str(*name) {
                        write!(w, "<native proc {}>", name)
                    } else {
                        write!(w, "<anonymous native proc>")
                    }
                }
                Expr::Procedure {
                    args,
                    body,
                    variadic,
                    is_lambda,
                    ..
                } => {
                    write!(w, "(")?;
                    if *is_lambda {
                        write!(w, "lambda (")?;
                    } else {
                        write!(w, "macro (")?;
                    }

                    if *variadic {}

                    let (draw_now_args, special) = if *variadic && args.last().is_some() {
                        (&args[..args.len() - 1], true)
                    } else {
                        (args.as_slice(), false)
                    };

                    for (idx, (sym, default)) in draw_now_args.iter().enumerate() {
                        let symbol = engine.get_symbol_str(*sym).unwrap_or("<unknown>");
                        if let Some(default) = default {
                            write!(w, "[{} ", symbol)?;
                            recur(engine, w, default.to_owned())?;
                            write!(w, "]")?;
                        } else {
                            write!(w, "{}", symbol)?;
                        }
                        if idx != draw_now_args.len() - 1 {
                            write!(w, " ")?;
                        }
                    }
                    if special {
                        // a little hacky but we can't have default trail args
                        let last = engine
                            .get_symbol_str(args.last().unwrap().0)
                            .unwrap_or("<unknown>");
                        write!(w, ". {}", last)?;
                    }
                    write!(w, ")")?;

                    for body_expr in body {
                        write!(w, " ")?;
                        recur(engine, w, body_expr.clone())?;
                    }

                    write!(w, ")")
                }
            }
        }
        let mut writer = String::new();
        recur(self, &mut writer, expr).unwrap();
        writer
    }

    /// Print the expression to a string
    /// in a nice and human-readable way.
    pub fn print_expr(&self, expr: Gc<Expr>) -> String {
        fn recur<W: Write>(engine: &Engine, w: &mut W, expr: Gc<Expr>) -> Result<(), fmt::Error> {
            match &*expr {
                Expr::Integer(i) => write!(w, "{}", i),
                Expr::Float(f) => write!(w, "{}", f),
                Expr::String(s) => {
                    write!(w, "{}", s)
                }
                Expr::Symbol(sym) => {
                    if let Some(s) = engine.get_symbol_str(*sym) {
                        write!(w, "{}", s)
                    } else {
                        write!(w, "<unknown #{}>", sym)
                    }
                }
                Expr::Pair(car, cdr) => {
                    fn write_list<W: Write>(
                        engine: &Engine,
                        w: &mut W,
                        car: Gc<Expr>,
                        cdr: Gc<Expr>,
                    ) -> Result<(), fmt::Error> {
                        recur(engine, w, car)?;
                        match &*cdr {
                            // Proper list's end, do nothing
                            Expr::Nil => Ok(()),
                            // Proper list, leave space for the next thing.
                            Expr::Pair(cdar, cddr) => {
                                write!(w, " ")?;
                                write_list(engine, w, cdar.to_owned(), cddr.to_owned())
                            }
                            // Just a pair
                            _ => {
                                write!(w, " . ")?;
                                recur(engine, w, cdr.to_owned())
                            }
                        }
                    }

                    write!(w, "(")?;
                    write_list(engine, w, car.clone(), cdr.clone())?;
                    write!(w, ")")
                }
                Expr::Nil => {
                    write!(w, "()")
                }
                Expr::SpecialForm { name, .. } => {
                    if let Some(name) = engine.get_symbol_str(*name) {
                        write!(w, "<native func {}>", name)
                    } else {
                        write!(w, "<anonymous native func>")
                    }
                }
                Expr::NativeProcedure { name, .. } => {
                    if let Some(name) = engine.get_symbol_str(*name) {
                        write!(w, "<native proc {}>", name)
                    } else {
                        write!(w, "<anonymous native proc>")
                    }
                }
                Expr::Procedure { .. } => {
                    write!(w, "<procedure>")
                }
            }
        }
        let mut writer = String::new();
        recur(self, &mut writer, expr).unwrap();
        writer
    }

    /// Make or get the symbol handle of the symbol represented by the given string.
    pub fn intern_symbol(&mut self, sym: &str) -> Symbol {
        if let Some(already) = self.interned_symbols.get_by_left(sym) {
            *already
        } else {
            let id = self.akashic_symbol_count;
            self.interned_symbols.insert(sym.to_string(), id);

            self.akashic_symbol_count += 1;
            id
        }
    }

    /// Create a symbol guaranteed to not have been seen before.
    /// Even if some overly-clever person tries to make a symbol with the same
    /// string content, it won't work, because this symbol will have an internal
    /// ID different from it.
    pub fn unique_symbol(&mut self) -> Symbol {
        let sym = format!("_uniq#{}", self.akashic_symbol_count);
        self.intern_symbol(&sym)
    }

    /// Get the ID of the already-existing symbol with the given name.
    pub fn find_symbol(&self, sym: &str) -> Option<Symbol> {
        self.interned_symbols.get_by_left(sym).copied()
    }

    pub fn get_symbol_str(&self, symbol_id: Symbol) -> Option<&str> {
        if let Some(sym) = self.interned_symbols.get_by_right(&symbol_id) {
            Some(sym.as_str())
        } else {
            None
        }
    }

    /// Turn a cons list into a vector of indices.
    /// If the given index or any cdr doesn't point to a `Pair`
    /// or `Null` (ie it's not a proper list)
    /// then `None` is returned.
    pub fn sexp_to_list(&self, expr: Gc<Expr>) -> Option<Vec<Gc<Expr>>> {
        let (list, end) = self.expr_to_improper_list(expr);
        if let Expr::Nil = &*end {
            Some(list)
        } else {
            None
        }
    }

    /// Turn an improper list into the list leading up to the last element,
    /// and the last element. Proper lists will have the last element be `()`.
    pub fn expr_to_improper_list(&self, expr: Gc<Expr>) -> (Vec<Gc<Expr>>, Gc<Expr>) {
        fn recur(engine: &Engine, expr: Gc<Expr>, wip: &mut Vec<Gc<Expr>>) -> Gc<Expr> {
            match &*expr {
                Expr::Pair(car, cdr) => {
                    wip.push(car.to_owned());
                    recur(engine, cdr.to_owned(), wip)
                }
                _ => expr,
            }
        }
        let mut out = Vec::new();
        let last = recur(self, expr, &mut out);
        (out, last)
    }

    /// Create a cons list from the given list, and return its head.
    pub fn list_to_sexp(&self, list: &[Gc<Expr>]) -> Gc<Expr> {
        if let Some((car, cdr)) = list.split_first() {
            Gc::new(Expr::Pair(car.clone(), self.list_to_sexp(cdr)))
        } else {
            Gc::new(Expr::Nil)
        }
    }

    pub fn is_truthy(&self, expr: Gc<Expr>) -> bool {
        match &*expr {
            Expr::Nil => false,
            Expr::Symbol(sym) => {
                let f = self.interned_symbols.get_by_left("false");
                if let Some(f) = f {
                    // If it equals "false" return false
                    // Otherwise true
                    f != sym
                } else {
                    // somehow undefined false
                    true
                }
            }
            // everything else is truthy
            _ => true,
        }
    }

    pub fn make_bool(&mut self, b: bool) -> Gc<Expr> {
        Gc::new(Expr::Symbol(self.intern_symbol(if b {
            "true"
        } else {
            "false"
        })))
    }

    /// Make an error, a cons list `'(! "msg")` or `'(! "msg" userdata)`.
    pub fn make_err(&mut self, msg: String, userdata: Option<Gc<Expr>>) -> Gc<Expr> {
        let oh_no = self.intern_symbol("!");
        if let Some(userdata) = userdata {
            self.list_to_sexp(&[
                Gc::new(Expr::Symbol(oh_no)),
                Gc::new(Expr::String(msg)),
                userdata,
            ])
        } else {
            self.list_to_sexp(&[Gc::new(Expr::Symbol(oh_no)), Gc::new(Expr::String(msg))])
        }
    }

    /// Get a reference to the engine's thtdlib.
    pub fn thtdlib(&self) -> Gc<GcCell<Namespace>> {
        self.thtdlib.clone()
    }
}

/// Mapping of symbols to places in memory.
#[derive(Debug, Clone, Trace, Finalize)]
pub struct Namespace {
    mappings: HashMap<Symbol, Gc<Expr>>,
    parent: Option<Gc<GcCell<Namespace>>>,
}

impl Namespace {
    pub fn new(parent: Gc<GcCell<Namespace>>) -> Self {
        Self {
            mappings: HashMap::new(),
            parent: Some(parent),
        }
    }

    pub fn insert(&mut self, symbol: Symbol, target: Gc<Expr>) {
        self.mappings.insert(symbol, target);
    }

    pub fn lookup(&self, symbol: Symbol) -> Option<Gc<Expr>> {
        self.mappings.get(&symbol).cloned().or_else(|| {
            self.parent
                .as_ref()
                .and_then(|parent| parent.borrow().lookup(symbol))
        })
    }
}
