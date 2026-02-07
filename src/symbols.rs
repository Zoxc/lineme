use std::collections::HashMap;
use std::sync::Arc;

/// A compact identifier for an interned string.
///
/// This is intentionally a small, copyable type (backed by `u32`) so it can be
/// cheaply stored in other data structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Symbol(u32);

impl std::fmt::Display for Symbol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "#{}", self.0)
    }
}

/// A very small string interner for application-wide symbols.
///
/// Not thread-safe — the application can keep a single `Symbols` instance and
/// pass mutable access where needed. Interning returns a `Symbol` which can be
/// cheaply copied and compared.
#[derive(Default, Debug, Clone)]
pub struct Symbols {
    map: HashMap<Arc<str>, Symbol>,
    vec: Vec<Arc<str>>,
}

impl Symbols {
    /// Create a new empty interner.
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            vec: Vec::new(),
        }
    }

    /// Intern `s` and return a `Symbol` referring to the stored string.
    ///
    /// If the string was already interned, the existing `Symbol` is returned.
    pub fn intern(&mut self, s: &str) -> Symbol {
        if let Some(&sym) = self.map.get(s) {
            return sym;
        }

        let arc: Arc<str> = s.into();
        let idx = self.vec.len() as u32;
        self.vec.push(arc.clone());
        let sym = Symbol(idx);
        self.map.insert(arc, sym);
        sym
    }

    /// Look up the interned string for `symbol`.
    ///
    /// Returns a `&str` referencing the interned string. Panics if the symbol
    /// is unknown — callers must only request symbols that were previously
    /// returned by `intern`.
    pub fn resolve(&self, symbol: Symbol) -> &str {
        self.vec.get(symbol.0 as usize).expect("unknown Symbol")
    }
}
