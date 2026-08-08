use std::borrow::Borrow;
use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use unicode_casefold::UnicodeCaseFold;
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Symbol(Arc<str>);

impl Symbol {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for Symbol {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("Symbol")
            .field(&self.as_str())
            .finish()
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Default)]
pub(crate) struct SymbolInterner {
    values: HashMap<String, Arc<str>>,
}

impl SymbolInterner {
    pub(crate) fn intern_identifier(&mut self, raw: &str) -> Result<Symbol, IdentifierError> {
        let normalized = normalize_identifier(raw)?;
        if let Some(existing) = self.values.get(normalized.as_str()) {
            return Ok(Symbol(existing.clone()));
        }
        let shared: Arc<str> = Arc::from(normalized.as_str());
        self.values.insert(normalized, shared.clone());
        Ok(Symbol(shared))
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentifierError {
    pub character_index: u64,
    pub character: char,
    pub expected: &'static str,
}

impl fmt::Display for IdentifierError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "invalid identifier character {:?} at character {}: expected {}",
            self.character, self.character_index, self.expected
        )
    }
}

impl std::error::Error for IdentifierError {}

/// Validates Unicode XID spelling and returns its canonical NFC form.
pub fn normalize_identifier(raw: &str) -> Result<String, IdentifierError> {
    let mut characters = raw.chars();
    let Some(first) = characters.next() else {
        return Err(IdentifierError {
            character_index: 0,
            character: '\0',
            expected: "Unicode XID_Start or `_`",
        });
    };
    if first != '_' && !unicode_ident::is_xid_start(first) {
        return Err(IdentifierError {
            character_index: 0,
            character: first,
            expected: "Unicode XID_Start or `_`",
        });
    }
    for (index, character) in characters.enumerate() {
        if character != '_' && !unicode_ident::is_xid_continue(character) {
            return Err(IdentifierError {
                character_index: u64::try_from(index).unwrap_or(u64::MAX).saturating_add(1),
                character,
                expected: "Unicode XID_Continue or `_`",
            });
        }
    }
    Ok(raw.nfc().collect())
}

/// Canonical filename-collision key: NFC, full non-Turkic case fold, then NFC.
pub fn case_fold_nfc(text: &str) -> String {
    text.nfc().case_fold().nfc().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_xid_identifiers_to_nfc() {
        assert_eq!(normalize_identifier("Cafe\u{301}").unwrap(), "Caf\u{e9}");
        assert_eq!(normalize_identifier("_delta2").unwrap(), "_delta2");
        assert!(normalize_identifier("2bad").is_err());
        assert!(normalize_identifier("bad-name").is_err());
    }

    #[test]
    fn collision_key_uses_full_case_folding() {
        assert_eq!(case_fold_nfc("Stra\u{df}e"), case_fold_nfc("STRASSE"));
        assert_eq!(case_fold_nfc("Cafe\u{301}"), case_fold_nfc("CAF\u{c9}"));
    }
}
