use std::borrow::Borrow;
use std::collections::HashSet;
use std::fmt;
use std::io;
use std::ops::Deref;
use std::sync::Arc;

/// An identifier whose UTF-8 storage is shared by every occurrence interned
/// by the same lexer or Core-name table.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Identifier(Arc<String>);

impl Identifier {
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    #[cfg(test)]
    pub fn shares_storage_with(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl AsRef<str> for Identifier {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for Identifier {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Deref for Identifier {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl fmt::Display for Identifier {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl PartialEq<str> for Identifier {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for Identifier {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

/// A phase-local lookup table. Returned identifiers own their shared backing
/// allocation, so they remain valid after the table itself is dropped.
#[derive(Debug, Default)]
pub struct IdentifierInterner {
    values: HashSet<Identifier>,
}

impl IdentifierInterner {
    pub fn intern(&mut self, text: String) -> io::Result<Identifier> {
        if let Some(existing) = self.values.get(text.as_str()) {
            return Ok(existing.clone());
        }

        self.values.try_reserve(1).map_err(|error| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("could not reserve identifier table memory: {error}"),
            )
        })?;
        let identifier = Identifier(Arc::new(text));
        self.values.insert(identifier.clone());
        Ok(identifier)
    }

    pub fn intern_str(&mut self, text: &str) -> io::Result<Identifier> {
        let mut owned = String::new();
        owned.try_reserve_exact(text.len()).map_err(|error| {
            io::Error::new(
                io::ErrorKind::OutOfMemory,
                format!("could not reserve identifier text memory: {error}"),
            )
        })?;
        owned.push_str(text);
        self.intern(owned)
    }
}

#[cfg(test)]
impl From<&str> for Identifier {
    fn from(text: &str) -> Self {
        Self(Arc::new(text.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_text_shares_one_backing_allocation() {
        let mut interner = IdentifierInterner::default();
        let first = interner.intern_str("Position").unwrap();
        let second = interner.intern("Position".to_owned()).unwrap();
        let other = interner.intern_str("Velocity").unwrap();

        assert!(first.shares_storage_with(&second));
        assert!(!first.shares_storage_with(&other));
        assert_eq!(first.as_str(), "Position");
    }

    #[test]
    fn identifier_storage_outlives_the_lookup_table() {
        let identifier = {
            let mut interner = IdentifierInterner::default();
            interner.intern_str("Persistent").unwrap()
        };

        assert_eq!(identifier.as_str(), "Persistent");
    }
}
