use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

const BASE: u32 = 26;
const MAX_SUFFIX: u32 = BASE * BASE * BASE;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Indicatif(String);

#[derive(Debug)]
pub enum IndicatifError {
    TooShort,
    Overflow(Indicatif),
}

impl fmt::Display for IndicatifError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooShort => write!(f, "indicatif trop court (< 3 caractères)"),
            Self::Overflow(i) => write!(f, "dépassement après {i}"),
        }
    }
}

impl std::error::Error for IndicatifError {}

impl Indicatif {
    pub fn new(s: impl Into<String>) -> Result<Self, IndicatifError> {
        let s = s.into();
        if s.len() < 3 {
            return Err(IndicatifError::TooShort);
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn next(&self) -> Result<Self, IndicatifError> {
        let len = self.0.len();
        let prefix = &self.0[..len - 3];
        let suffix = &self.0[len - 3..];
        let n = suffix_to_int(suffix) + 1;
        if n >= MAX_SUFFIX {
            return Err(IndicatifError::Overflow(self.clone()));
        }
        Ok(Self(format!("{prefix}{}", int_to_suffix(n))))
    }

    pub fn range(start: Self, end: Self) -> IndicatifRange {
        IndicatifRange {
            next: Some(start),
            end,
        }
    }
}

impl fmt::Display for Indicatif {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for Indicatif {
    type Err = IndicatifError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new(s)
    }
}

impl TryFrom<String> for Indicatif {
    type Error = IndicatifError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::new(s)
    }
}

impl From<Indicatif> for String {
    fn from(i: Indicatif) -> Self {
        i.0
    }
}

pub struct IndicatifRange {
    next: Option<Indicatif>,
    end: Indicatif,
}

impl Iterator for IndicatifRange {
    type Item = Indicatif;
    fn next(&mut self) -> Option<Self::Item> {
        let current = self.next.take()?;
        if current != self.end {
            self.next = current.next().ok();
        }
        Some(current)
    }
}

/// Convertit un suffixe ASCII majuscule (typiquement 3 lettres) en entier base-26.
/// Suppose l'entrée valide ; aucun panic mais le résultat est indéfini si
/// caractères non-`A`..`Z`.
pub fn suffix_to_int(suffix: &str) -> u32 {
    suffix
        .chars()
        .fold(0, |acc, c| acc * BASE + (c as u32 - 'A' as u32))
}

/// Inverse de [`suffix_to_int`] sur 3 positions.
pub fn int_to_suffix(n: u32) -> String {
    let to_char = |d: u32| char::from_u32('A' as u32 + d).unwrap();
    format!(
        "{}{}{}",
        to_char(n / (BASE * BASE)),
        to_char(n / BASE % BASE),
        to_char(n % BASE),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn next_within_letter() {
        let i = Indicatif::new("F4AAA").unwrap();
        assert_eq!(i.next().unwrap().as_str(), "F4AAB");
    }

    #[test]
    fn next_carries_letter() {
        let i = Indicatif::new("F4AAZ").unwrap();
        assert_eq!(i.next().unwrap().as_str(), "F4ABA");
    }

    #[test]
    fn next_carries_twice() {
        let i = Indicatif::new("F4AZZ").unwrap();
        assert_eq!(i.next().unwrap().as_str(), "F4BAA");
    }

    #[test]
    fn next_overflow_zzz() {
        let i = Indicatif::new("F4ZZZ").unwrap();
        assert!(matches!(i.next(), Err(IndicatifError::Overflow(_))));
    }

    #[test]
    fn range_inclusive_small() {
        let v: Vec<String> = Indicatif::range(
            Indicatif::new("F4AAA").unwrap(),
            Indicatif::new("F4AAC").unwrap(),
        )
        .map(|i| i.as_str().to_string())
        .collect();
        assert_eq!(v, vec!["F4AAA", "F4AAB", "F4AAC"]);
    }

    #[test]
    fn range_singleton() {
        let v: Vec<_> = Indicatif::range(
            Indicatif::new("F4AAA").unwrap(),
            Indicatif::new("F4AAA").unwrap(),
        )
        .collect();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].as_str(), "F4AAA");
    }

    #[test]
    fn range_stops_on_overflow() {
        // Si on dépasse ZZZ sans atteindre end, l'itération s'arrête.
        let v: Vec<_> = Indicatif::range(
            Indicatif::new("F4ZZY").unwrap(),
            Indicatif::new("F5AAA").unwrap(),
        )
        .collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].as_str(), "F4ZZZ");
    }

    #[test]
    fn rejects_too_short() {
        assert!(matches!(Indicatif::new("AB"), Err(IndicatifError::TooShort)));
    }

    #[test]
    fn ordering() {
        let a = Indicatif::new("F4AAA").unwrap();
        let b = Indicatif::new("F4AAB").unwrap();
        let c = Indicatif::new("F5AAA").unwrap();
        assert!(a < b);
        assert!(b < c);
    }

    #[test]
    fn serde_roundtrip() {
        let i = Indicatif::new("F4ABC").unwrap();
        let json = serde_json::to_string(&i).unwrap();
        assert_eq!(json, "\"F4ABC\"");
        let back: Indicatif = serde_json::from_str(&json).unwrap();
        assert_eq!(back, i);
    }

    #[test]
    fn suffix_int_roundtrip() {
        for s in ["AAA", "AAZ", "ABA", "ZAA", "ZZZ", "MNO"] {
            assert_eq!(int_to_suffix(suffix_to_int(s)), s);
        }
    }
}
