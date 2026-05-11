use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use scraper::{Html, Selector};
use serde::{Deserialize, Serialize};

use crate::indicatif::Indicatif;

static ROW_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("tr").unwrap());
static CELL_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("td").unwrap());

/// Marqueur ANFR signalant un opérateur sur liste rouge (présent en base mais
/// adresse non publique).
pub const LISTE_ORANGE: &str = "LISTE ORANGE";

/// Une cellule ANFR vaut "blanche" si vide ou égale au tiret-marqueur `"-"`.
pub fn is_blank(s: &str) -> bool {
    s.is_empty() || s == "-"
}

/// Trim + uppercase d'un champ optionnel ; rend `None` si vide ou marqueur "-".
pub fn normalize_token(s: Option<&str>) -> Option<String> {
    let s = s?.trim().to_uppercase();
    (!is_blank(&s)).then_some(s)
}

fn epoch() -> DateTime<Utc> {
    DateTime::<Utc>::from_timestamp(0, 0).unwrap()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub indicatif: Indicatif,
    pub nom: Option<String>,
    pub prenom: Option<String>,
    pub adresse1: Option<String>,
    pub adresse2: Option<String>,
    pub localite: Option<String>,
    pub code_postal: Option<String>,
    #[serde(default = "epoch")]
    pub last_checked: DateTime<Utc>,
}

fn row_to_record(cells: Vec<String>) -> Option<Record> {
    let [indicatif, nom, prenom, adresse1, adresse2, localite, code_postal]: [String; 7] =
        cells.try_into().ok()?;
    Some(Record {
        indicatif: Indicatif::new(indicatif).ok()?,
        nom: Some(nom),
        prenom: Some(prenom),
        adresse1: Some(adresse1),
        adresse2: Some(adresse2),
        localite: Some(localite),
        code_postal: Some(code_postal),
        last_checked: Utc::now(),
    })
}

pub fn is_waf_block(html: &str) -> bool {
    html.contains("Demande rejet") || html.contains("support ID")
}

pub fn is_session_expired(html: &str) -> bool {
    html.contains("session a pris fin") || html.contains("t-PageBody--login")
}

pub fn parse_rows(html: &str) -> Vec<Record> {
    let doc = Html::parse_document(html);
    doc.select(&ROW_SELECTOR)
        .filter_map(|tr| {
            let cells: Vec<String> = tr
                .select(&CELL_SELECTOR)
                .map(|td| td.text().collect::<String>().trim().to_string())
                .collect();
            row_to_record(cells)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waf_detection() {
        assert!(is_waf_block("<p>Demande rejetée par le pare-feu</p>"));
        assert!(is_waf_block("<p>Veuillez contacter le support ID: 42</p>"));
        assert!(!is_waf_block("<table></table>"));
        assert!(!is_waf_block(""));
    }

    #[test]
    fn session_expired_detection() {
        assert!(is_session_expired("Votre session a pris fin."));
        assert!(is_session_expired("<body class=\"t-PageBody--login\">"));
        assert!(!is_session_expired("<table></table>"));
    }

    #[test]
    fn parse_one_row() {
        let html = "<html><table><tr>\
            <td>F4ABC</td><td>DUPONT</td><td>JEAN</td>\
            <td>1 RUE X</td><td>BAT A</td><td>PARIS</td><td>75001</td>\
            </tr></table></html>";
        let rows = parse_rows(html);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].indicatif.as_str(), "F4ABC");
        assert_eq!(rows[0].nom.as_deref(), Some("DUPONT"));
        assert_eq!(rows[0].prenom.as_deref(), Some("JEAN"));
        assert_eq!(rows[0].code_postal.as_deref(), Some("75001"));
    }

    #[test]
    fn parse_skips_malformed() {
        // 6 cellules au lieu de 7
        let html = "<table><tr>\
            <td>F4ABC</td><td>NOM</td><td>P</td><td>A</td><td>B</td><td>C</td>\
            </tr></table>";
        assert!(parse_rows(html).is_empty());
    }

    #[test]
    fn parse_skips_invalid_indicatif() {
        // indicatif < 3 caractères
        let html = "<table><tr>\
            <td>F</td><td>NOM</td><td>P</td><td>A</td><td>B</td><td>C</td><td>75001</td>\
            </tr></table>";
        assert!(parse_rows(html).is_empty());
    }

    #[test]
    fn trims_whitespace() {
        let html = "<table><tr>\
            <td>  F4ABC  </td><td>DUPONT</td><td>JEAN</td>\
            <td>RUE</td><td></td><td>PARIS</td><td>75001</td>\
            </tr></table>";
        let rows = parse_rows(html);
        assert_eq!(rows[0].indicatif.as_str(), "F4ABC");
        assert_eq!(rows[0].adresse2.as_deref(), Some(""));
    }
}
