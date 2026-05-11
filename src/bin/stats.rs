use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::LazyLock;

use anfr_scrape::html_parse::{self, Record, LISTE_ORANGE};
use anfr_scrape::state;
use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;

const HTML_TEMPLATE: &str = include_str!("stats_template.html");
const PLACEHOLDER: &str = "__STATS_JSON__";

/// Classes de préfixe française radioamateur ; affichées dans la stat même
/// avec 0 record (pour signaler ce qui reste à scraper).
const EXPECTED_PREFIXES: &[&str] = &["F0", "F1", "F4", "F5", "F6", "F8"];

#[derive(Parser, Debug)]
#[command(about = "Génère un dashboard HTML de statistiques sur les indicatifs")]
struct Args {
    #[arg(long, default_value = "data/indicatifs.json", help = "fichier des opérateurs")]
    input: PathBuf,

    #[arg(long, default_value = "dist/stats.html", help = "fichier HTML de sortie")]
    output: PathBuf,
}

#[derive(Serialize)]
struct Totals {
    active: usize,
    geocodable: usize,
    liste_orange: usize,
    feminine: u32,
}

#[derive(Serialize)]
struct PrefixStat {
    prefix: String,
    active: u32,
}

#[derive(Serialize)]
struct Bucket {
    label: String,
    count: u32,
}

#[derive(Serialize)]
struct DeptStat {
    code: String,
    name: String,
    count: u32,
}

#[derive(Serialize)]
struct Payload {
    generated_at: String,
    totals: Totals,
    by_prefix: Vec<PrefixStat>,
    by_region: Vec<Bucket>,
    top_depts: Vec<DeptStat>,
    top_communes: Vec<Bucket>,
    top_prenoms: Vec<Bucket>,
}

fn dept_code(cp: &str) -> Option<String> {
    if cp.len() != 5 || !cp.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let first2 = &cp[0..2];
    if first2 == "97" || first2 == "98" {
        Some(cp[0..3].to_string())
    } else {
        Some(first2.to_string())
    }
}

#[allow(clippy::too_many_lines)]
fn dept_name(code: &str) -> &'static str {
    match code {
        "01" => "Ain",
        "02" => "Aisne",
        "03" => "Allier",
        "04" => "Alpes-de-Haute-Provence",
        "05" => "Hautes-Alpes",
        "06" => "Alpes-Maritimes",
        "07" => "Ardèche",
        "08" => "Ardennes",
        "09" => "Ariège",
        "10" => "Aube",
        "11" => "Aude",
        "12" => "Aveyron",
        "13" => "Bouches-du-Rhône",
        "14" => "Calvados",
        "15" => "Cantal",
        "16" => "Charente",
        "17" => "Charente-Maritime",
        "18" => "Cher",
        "19" => "Corrèze",
        "20" => "Corse",
        "21" => "Côte-d'Or",
        "22" => "Côtes-d'Armor",
        "23" => "Creuse",
        "24" => "Dordogne",
        "25" => "Doubs",
        "26" => "Drôme",
        "27" => "Eure",
        "28" => "Eure-et-Loir",
        "29" => "Finistère",
        "30" => "Gard",
        "31" => "Haute-Garonne",
        "32" => "Gers",
        "33" => "Gironde",
        "34" => "Hérault",
        "35" => "Ille-et-Vilaine",
        "36" => "Indre",
        "37" => "Indre-et-Loire",
        "38" => "Isère",
        "39" => "Jura",
        "40" => "Landes",
        "41" => "Loir-et-Cher",
        "42" => "Loire",
        "43" => "Haute-Loire",
        "44" => "Loire-Atlantique",
        "45" => "Loiret",
        "46" => "Lot",
        "47" => "Lot-et-Garonne",
        "48" => "Lozère",
        "49" => "Maine-et-Loire",
        "50" => "Manche",
        "51" => "Marne",
        "52" => "Haute-Marne",
        "53" => "Mayenne",
        "54" => "Meurthe-et-Moselle",
        "55" => "Meuse",
        "56" => "Morbihan",
        "57" => "Moselle",
        "58" => "Nièvre",
        "59" => "Nord",
        "60" => "Oise",
        "61" => "Orne",
        "62" => "Pas-de-Calais",
        "63" => "Puy-de-Dôme",
        "64" => "Pyrénées-Atlantiques",
        "65" => "Hautes-Pyrénées",
        "66" => "Pyrénées-Orientales",
        "67" => "Bas-Rhin",
        "68" => "Haut-Rhin",
        "69" => "Rhône",
        "70" => "Haute-Saône",
        "71" => "Saône-et-Loire",
        "72" => "Sarthe",
        "73" => "Savoie",
        "74" => "Haute-Savoie",
        "75" => "Paris",
        "76" => "Seine-Maritime",
        "77" => "Seine-et-Marne",
        "78" => "Yvelines",
        "79" => "Deux-Sèvres",
        "80" => "Somme",
        "81" => "Tarn",
        "82" => "Tarn-et-Garonne",
        "83" => "Var",
        "84" => "Vaucluse",
        "85" => "Vendée",
        "86" => "Vienne",
        "87" => "Haute-Vienne",
        "88" => "Vosges",
        "89" => "Yonne",
        "90" => "Territoire de Belfort",
        "91" => "Essonne",
        "92" => "Hauts-de-Seine",
        "93" => "Seine-Saint-Denis",
        "94" => "Val-de-Marne",
        "95" => "Val-d'Oise",
        "971" => "Guadeloupe",
        "972" => "Martinique",
        "973" => "Guyane",
        "974" => "La Réunion",
        "975" => "Saint-Pierre-et-Miquelon",
        "976" => "Mayotte",
        "977" => "Saint-Barthélemy",
        "978" => "Saint-Martin",
        "984" => "TAAF",
        "986" => "Wallis-et-Futuna",
        "987" => "Polynésie française",
        "988" => "Nouvelle-Calédonie",
        _ => "Inconnu",
    }
}

/// Région INSEE post-2016 pour un code département (2 ou 3 caractères).
/// DOM/COM : chaque territoire est sa propre "région".
fn region_for(code: &str) -> &'static str {
    match code {
        "01" | "03" | "07" | "15" | "26" | "38" | "42" | "43" | "63" | "69" | "73" | "74" => {
            "Auvergne-Rhône-Alpes"
        }
        "21" | "25" | "39" | "58" | "70" | "71" | "89" | "90" => "Bourgogne-Franche-Comté",
        "22" | "29" | "35" | "56" => "Bretagne",
        "18" | "28" | "36" | "37" | "41" | "45" => "Centre-Val de Loire",
        "20" => "Corse",
        "08" | "10" | "51" | "52" | "54" | "55" | "57" | "67" | "68" | "88" => "Grand Est",
        "02" | "59" | "60" | "62" | "80" => "Hauts-de-France",
        "75" | "77" | "78" | "91" | "92" | "93" | "94" | "95" => "Île-de-France",
        "14" | "27" | "50" | "61" | "76" => "Normandie",
        "16" | "17" | "19" | "23" | "24" | "33" | "40" | "47" | "64" | "79" | "86" | "87" => {
            "Nouvelle-Aquitaine"
        }
        "09" | "11" | "12" | "30" | "31" | "32" | "34" | "46" | "48" | "65" | "66" | "81"
        | "82" => "Occitanie",
        "44" | "49" | "53" | "72" | "85" => "Pays de la Loire",
        "04" | "05" | "06" | "13" | "83" | "84" => "Provence-Alpes-Côte d'Azur",
        // DOM / COM : territoire = région
        "971" | "972" | "973" | "974" | "975" | "976" | "977" | "978" | "984" | "986" | "987"
        | "988" => dept_name(code),
        _ => "Inconnu",
    }
}

fn split_indicatif(ind: &str) -> Option<(String, String)> {
    if ind.len() != 5 {
        return None;
    }
    Some((ind[0..2].to_string(), ind[2..5].to_string()))
}

/// Normalise un prénom français : majuscules + suppression des diacritiques
/// usuels (É→E, È→E, etc.). Utilisé seulement pour matcher la liste de prénoms
/// féminins ; l'affichage conserve l'orthographe d'origine.
fn strip_accents(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'É' | 'È' | 'Ê' | 'Ë' => 'E',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'À' | 'Â' | 'Ä' => 'A',
            'à' | 'â' | 'ä' => 'a',
            'Î' | 'Ï' => 'I',
            'î' | 'ï' => 'i',
            'Ô' | 'Ö' => 'O',
            'ô' | 'ö' => 'o',
            'Ù' | 'Û' | 'Ü' => 'U',
            'ù' | 'û' | 'ü' => 'u',
            'Ç' => 'C',
            'ç' => 'c',
            c => c,
        })
        .collect()
}

/// Liste curée de prénoms féminins français usuels (formes sans diacritique,
/// majuscules). Le décompte sous-estime nécessairement les prénoms rares,
/// étrangers ou neutres (Dominique, Camille, Claude) — *estimation*.
static FEMININE_NAMES: LazyLock<HashSet<&'static str>> = LazyLock::new(|| {
    [
        "AGATHE", "AGNES", "ALEXANDRA", "ALICE", "ALINE", "AMANDINE", "AMELIE", "ANAIS",
        "ANDREE", "ANGELE", "ANITA", "ANNE", "ANNETTE", "ANNICK", "ANNIE", "ANTOINETTE",
        "ARLETTE", "AUDE", "AUDREY", "AURELIE", "AURORE",
        "BEATRICE", "BERNADETTE", "BERTHE", "BLANCHE", "BRIGITTE",
        "CAROLE", "CAROLINE", "CATHERINE", "CECILE", "CELINE", "CHANTAL", "CHARLOTTE",
        "CHRISTELLE", "CHRISTIANE", "CHRISTINE", "CHLOE", "CINDY", "CLAIRE", "CLARA",
        "CLAUDETTE", "CLAUDINE", "COLETTE", "CORINNE",
        "DANIELLE", "DELPHINE", "DENISE", "DIANE",
        "EDITH", "ELIANE", "ELISABETH", "ELISE", "ELODIE", "EMILIE", "EMMA", "EMMANUELLE",
        "ESTELLE", "EUGENIE", "EVE", "EVELYNE",
        "FABIENNE", "FANNY", "FATIMA", "FELICIE", "FLORENCE", "FRANCETTE", "FRANCINE",
        "FRANCOISE",
        "GABRIELLE", "GAELLE", "GENEVIEVE", "GEORGETTE", "GERMAINE", "GHISLAINE",
        "GHYSLAINE", "GINETTE", "GISELE",
        "HELENE", "HENRIETTE", "HUGUETTE",
        "INES", "IRENE", "ISABELLE",
        "JACQUELINE", "JEANINE", "JEANNE", "JEANNETTE", "JEANNINE", "JOELLE", "JOSEPHINE",
        "JOSETTE", "JOSIANE", "JULIE", "JULIETTE", "JUSTINE",
        "KARINE", "KATIA",
        "LAETITIA", "LAURE", "LAURENCE", "LEA", "LEONIE", "LILIANE", "LISE", "LOLA",
        "LOUISE", "LUCETTE", "LUCIE", "LUCIENNE", "LUDIVINE",
        "MADELEINE", "MAGALI", "MAGALY", "MANON", "MARCELLE", "MARGOT", "MARGUERITE",
        "MARIANNE", "MARIE", "MARION", "MARLENE", "MARTHE", "MARTINE", "MARYLINE",
        "MARYLISE", "MARYSE", "MARYVONNE", "MATHILDE", "MAUD", "MELANIE", "MICHELE",
        "MICHELINE", "MICHELLE", "MIREILLE", "MONA", "MONIQUE", "MURIEL", "MURIELLE",
        "MYLENE", "MYRIAM",
        "NADEGE", "NADIA", "NADINE", "NATHALIE", "NICOLE", "NOELLE", "NOEMIE",
        "ODETTE", "ODILE", "OLGA", "OLIVIA",
        "PASCALE", "PATRICIA", "PAULE", "PAULETTE", "PAULINE", "PERRINE", "PRISCILLA",
        "RACHEL", "RAYMONDE", "REINE", "RENEE", "ROSE", "ROSELYNE", "ROSEMARIE",
        "ROSETTE", "ROSITA", "ROXANE",
        "SABINE", "SABRINA", "SANDRA", "SANDRINE", "SARAH", "SEGOLENE", "SEVERINE",
        "SIMONE", "SOLANGE", "SOLENE", "SOLINE", "SONIA", "SOPHIE", "STEPHANIE",
        "SUZANNE", "SUZETTE", "SYLVIANE", "SYLVIE",
        "THERESE",
        "URSULE",
        "VALENTINE", "VALERIE", "VANESSA", "VERONIQUE", "VIRGINIE", "VIVIANE",
        "YOLANDE", "YVETTE", "YVONNE",
        "ZOE",
    ]
    .into_iter()
    .collect()
});

/// Vrai si le premier composant du prénom (avant un éventuel `-`) figure dans
/// la liste féminine. Heuristique : faux pour les composés majoritairement
/// masculins (`JEAN-MARIE`), vrai pour les composés féminins usuels (`MARIE-CLAUDE`,
/// `ANNE-LAURE`).
fn is_feminine(name: &str) -> bool {
    let first = name.split('-').next().unwrap_or("");
    let normalized = strip_accents(first);
    FEMININE_NAMES.contains(normalized.as_str())
}

#[derive(Default)]
struct Counters {
    active_by_prefix: BTreeMap<String, u32>,
    prenom_counts: HashMap<String, u32>,
    dept_counts: HashMap<String, u32>,
    region_counts: HashMap<String, u32>,
    commune_counts: HashMap<String, u32>,
    feminine: u32,
    liste_orange: usize,
    geocodable: usize,
}

fn count_records(records: &[Record]) -> Counters {
    let mut c = Counters::default();

    for r in records {
        if let Some((prefix, _)) = split_indicatif(r.indicatif.as_str()) {
            *c.active_by_prefix.entry(prefix).or_default() += 1;
        }

        if let Some(p) = html_parse::normalize_token(r.prenom.as_deref()) {
            if is_feminine(&p) {
                c.feminine += 1;
            }
            *c.prenom_counts.entry(p).or_default() += 1;
        }

        let is_orange = r.adresse1.as_deref() == Some(LISTE_ORANGE);
        if is_orange {
            c.liste_orange += 1;
        }

        // géocodage : skip vides, orange, et CP invalides
        if r.nom.is_none() || is_orange {
            continue;
        }
        let Some(cp) = r.code_postal.as_deref() else {
            continue;
        };
        if html_parse::is_blank(cp) {
            continue;
        }

        if let Some(code) = dept_code(cp) {
            *c.dept_counts.entry(code.clone()).or_default() += 1;
            *c.region_counts
                .entry(region_for(&code).to_string())
                .or_default() += 1;
            c.geocodable += 1;
        }
        if let Some(commune) = html_parse::normalize_token(r.localite.as_deref()) {
            *c.commune_counts.entry(commune).or_default() += 1;
        }
    }

    c
}

/// `by_prefix` inclut toujours `EXPECTED_PREFIXES` même à 0, plus tout préfixe
/// inattendu déjà présent dans les records. Trié alphabétiquement.
fn build_prefix_stats(active: &BTreeMap<String, u32>) -> Vec<PrefixStat> {
    let mut keys: Vec<&str> = EXPECTED_PREFIXES.to_vec();
    for k in active.keys() {
        if !keys.contains(&k.as_str()) {
            keys.push(k);
        }
    }
    keys.sort_unstable();
    keys.into_iter()
        .map(|p| PrefixStat {
            prefix: p.to_string(),
            active: active.get(p).copied().unwrap_or(0),
        })
        .collect()
}

fn build_dept_stats(dept_counts: HashMap<String, u32>) -> Vec<DeptStat> {
    let mut v: Vec<DeptStat> = dept_counts
        .into_iter()
        .map(|(code, count)| DeptStat {
            name: dept_name(&code).to_string(),
            code,
            count,
        })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.code.cmp(&b.code)));
    v
}

fn top_n(map: &HashMap<String, u32>, n: usize) -> Vec<Bucket> {
    let mut v: Vec<Bucket> = map
        .iter()
        .map(|(k, c)| Bucket {
            label: k.clone(),
            count: *c,
        })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    v.truncate(n);
    v
}

fn all_buckets(map: HashMap<String, u32>) -> Vec<Bucket> {
    let mut v: Vec<Bucket> = map
        .into_iter()
        .map(|(label, count)| Bucket { label, count })
        .collect();
    v.sort_by(|a, b| b.count.cmp(&a.count).then(a.label.cmp(&b.label)));
    v
}

fn compute(records: &[Record]) -> Payload {
    let c = count_records(records);

    let by_prefix = build_prefix_stats(&c.active_by_prefix);
    let by_region = all_buckets(c.region_counts);
    let top_depts = build_dept_stats(c.dept_counts);
    let top_communes = top_n(&c.commune_counts, 30);
    let top_prenoms = top_n(&c.prenom_counts, 30);

    Payload {
        generated_at: chrono::Utc::now().to_rfc3339(),
        totals: Totals {
            active: records.len(),
            geocodable: c.geocodable,
            liste_orange: c.liste_orange,
            feminine: c.feminine,
        },
        by_prefix,
        by_region,
        top_depts,
        top_communes,
        top_prenoms,
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    let records: Vec<Record> = state::read_json(&args.input, "records")?;
    eprintln!("{} enregistrements chargés", records.len());

    let payload = compute(&records);

    let json = serde_json::to_string(&payload)?;
    let html = HTML_TEMPLATE.replace(PLACEHOLDER, &json);
    std::fs::write(&args.output, html)
        .with_context(|| format!("écriture {}", args.output.display()))?;
    eprintln!("→ {}", args.output.display());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dept_code_metropole() {
        assert_eq!(dept_code("75001").as_deref(), Some("75"));
        assert_eq!(dept_code("06600").as_deref(), Some("06"));
        assert_eq!(dept_code("01000").as_deref(), Some("01"));
    }

    #[test]
    fn dept_code_outre_mer() {
        assert_eq!(dept_code("97400").as_deref(), Some("974"));
        assert_eq!(dept_code("98800").as_deref(), Some("988"));
    }

    #[test]
    fn dept_code_rejects_invalid() {
        assert_eq!(dept_code("ABCDE"), None);
        assert_eq!(dept_code("1234"), None);
        assert_eq!(dept_code("123456"), None);
        assert_eq!(dept_code(""), None);
    }

    #[test]
    fn split_indicatif_rejects_wrong_len() {
        assert!(split_indicatif("F4AB").is_none());
        assert!(split_indicatif("F4ABCD").is_none());
        assert!(split_indicatif("").is_none());
    }

    #[test]
    fn region_mapping_metropole() {
        assert_eq!(region_for("75"), "Île-de-France");
        assert_eq!(region_for("69"), "Auvergne-Rhône-Alpes");
        assert_eq!(region_for("13"), "Provence-Alpes-Côte d'Azur");
        assert_eq!(region_for("20"), "Corse");
        assert_eq!(region_for("35"), "Bretagne");
    }

    #[test]
    fn region_mapping_dom_com() {
        // DOM/COM : la région = le territoire lui-même
        assert_eq!(region_for("974"), "La Réunion");
        assert_eq!(region_for("988"), "Nouvelle-Calédonie");
    }

    #[test]
    fn region_mapping_unknown() {
        assert_eq!(region_for("ZZ"), "Inconnu");
        assert_eq!(region_for(""), "Inconnu");
    }

    #[test]
    fn strip_accents_basic() {
        assert_eq!(strip_accents("ÉLODIE"), "ELODIE");
        assert_eq!(strip_accents("Cécile"), "Cecile");
        assert_eq!(strip_accents("FRANÇOISE"), "FRANCOISE");
        assert_eq!(strip_accents("HÉLÈNE"), "HELENE");
        assert_eq!(strip_accents("PIERRE"), "PIERRE"); // pass-through
    }

    #[test]
    fn is_feminine_simple() {
        assert!(is_feminine("MARIE"));
        assert!(is_feminine("FRANCOISE"));
        assert!(is_feminine("FRANÇOISE")); // diacritique accepté
        assert!(is_feminine("HELENE"));
        assert!(is_feminine("HÉLÈNE"));
    }

    #[test]
    fn is_feminine_masculine_rejected() {
        assert!(!is_feminine("MICHEL"));
        assert!(!is_feminine("PIERRE"));
        assert!(!is_feminine("ALAIN"));
        // ambigus volontairement exclus de la liste — false négatif assumé
        assert!(!is_feminine("DOMINIQUE"));
        assert!(!is_feminine("CAMILLE"));
        assert!(!is_feminine("CLAUDE"));
    }

    #[test]
    fn is_feminine_compound_first_token() {
        // composé féminin : premier token féminin → True
        assert!(is_feminine("MARIE-CLAUDE"));
        assert!(is_feminine("ANNE-LAURE"));
        // composé masculin : premier token masculin → False
        assert!(!is_feminine("JEAN-MARIE"));
        assert!(!is_feminine("PIERRE-YVES"));
    }

    #[test]
    fn build_prefix_stats_includes_expected_zero() {
        let active: BTreeMap<String, u32> = [("F4".to_string(), 100)].into_iter().collect();
        let stats = build_prefix_stats(&active);
        let labels: Vec<&str> = stats.iter().map(|s| s.prefix.as_str()).collect();
        // F0 et autres doivent apparaître même à 0
        for expected in EXPECTED_PREFIXES {
            assert!(labels.contains(expected), "prefix {expected} absent");
        }
        let f4 = stats.iter().find(|s| s.prefix == "F4").unwrap();
        assert_eq!(f4.active, 100);
        let f0 = stats.iter().find(|s| s.prefix == "F0").unwrap();
        assert_eq!(f0.active, 0);
    }
}
