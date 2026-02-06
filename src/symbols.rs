use std::collections::HashMap;

/// A compact identifier for an interned string.
///
/// This is intentionally a small, copyable type (backed by `u32`) so it can be
/// cheaply stored in other data structures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Symbol(u32);

impl Default for Symbol {
    fn default() -> Self {
        Symbol(0)
    }
}

impl Symbol {
    /// Return the underlying index.
    pub fn index(self) -> u32 {
        self.0
    }
}

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
#[derive(Default, Debug)]
pub struct Symbols {
    map: HashMap<String, Symbol>,
    vec: Vec<String>,
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

        let owned = s.to_string();
        let idx = self.vec.len() as u32;
        self.vec.push(owned.clone());
        let sym = Symbol(idx);
        // We store a clone as the key; the string in `vec` owns the same contents
        // but keeping the map key simplifies lookup by &str.
        self.map.insert(owned, sym);
        sym
    }

    /// Look up the interned string for `symbol`.
    pub fn resolve(&self, symbol: Symbol) -> Option<&str> {
        self.vec.get(symbol.0 as usize).map(|s| s.as_str())
    }

    /// Return the number of unique interned strings.
    pub fn len(&self) -> usize {
        self.vec.len()
    }

    /// Return true if the interner contains no strings.
    pub fn is_empty(&self) -> bool {
        self.vec.is_empty()
    }
}
