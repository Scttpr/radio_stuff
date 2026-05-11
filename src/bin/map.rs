use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::Duration;

use anfr_scrape::html_parse::{self, Record, LISTE_ORANGE};
use anfr_scrape::state;
use anyhow::{Context, Result};
use clap::Parser;
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};

const HTML_TEMPLATE: &str = include_str!("map_template.html");
const PLACEHOLDER: &str = "__POINTS_JSON__";
const GEO_API: &str = "https://geo.api.gouv.fr/communes";

#[derive(Parser, Debug)]
#[command(about = "Génère une carte HTML des opérateurs par code postal")]
struct Args {
    #[arg(long, default_value = "data/indicatifs.json", help = "fichier source")]
    input: PathBuf,

    #[arg(long, default_value = "dist/map.html", help = "fichier HTML de sortie")]
    output: PathBuf,

    #[arg(long, default_value = "data/cp_geo.json", help = "cache CP → coordonnées")]
    cache: PathBuf,

    #[arg(long, default_value_t = 50, help = "délai entre appels géo (ms)")]
    delay_ms: u64,
}

#[derive(Serialize, Deserialize, Clone)]
struct GeoEntry {
    commune: String,
    lat: f64,
    lon: f64,
}

#[derive(Serialize)]
struct Point {
    cp: String,
    commune: String,
    lat: f64,
    lon: f64,
    indicatifs: Vec<String>,
}

#[derive(Deserialize)]
struct CommuneCentre {
    coordinates: [f64; 2],
}

#[derive(Deserialize)]
struct CommuneResponse {
    nom: String,
    centre: CommuneCentre,
}

fn fetch_cp(client: &Client, cp: &str) -> Result<Option<GeoEntry>> {
    let url = format!("{GEO_API}?codePostal={cp}&fields=nom,centre&format=json");
    let body = client.get(&url).send()?.error_for_status()?.text()?;
    let arr: Vec<CommuneResponse> = serde_json::from_str(&body)?;
    let Some(first) = arr.into_iter().next() else {
        return Ok(None);
    };
    Ok(Some(GeoEntry {
        commune: first.nom,
        lat: first.centre.coordinates[1],
        lon: first.centre.coordinates[0],
    }))
}

fn group_records(records: &[Record]) -> BTreeMap<String, Vec<String>> {
    let mut groups: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for r in records {
        let Some(cp) = r.code_postal.as_deref() else {
            continue;
        };
        if html_parse::is_blank(cp) {
            continue;
        }
        if r.nom.is_none() {
            continue;
        }
        if r.adresse1.as_deref() == Some(LISTE_ORANGE) {
            continue;
        }
        groups
            .entry(cp.to_string())
            .or_default()
            .push(r.indicatif.to_string());
    }
    for v in groups.values_mut() {
        v.sort();
    }
    groups
}

fn main() -> Result<()> {
    let args = Args::parse();

    let records: Vec<Record> = state::read_json(&args.input, "records")?;
    eprintln!("{} enregistrements chargés", records.len());

    let groups = group_records(&records);
    eprintln!("{} codes postaux après filtrage", groups.len());

    let mut cache: HashMap<String, GeoEntry> = state::read_json_or_default(&args.cache, "cache")?;
    let client = Client::builder()
        .user_agent("anfr_map/0.1")
        .timeout(Duration::from_secs(10))
        .build()?;

    let to_fetch: Vec<&String> = groups.keys().filter(|cp| !cache.contains_key(*cp)).collect();
    eprintln!(
        "{} CP à géocoder ({} déjà en cache)",
        to_fetch.len(),
        cache.len()
    );

    for (i, cp) in to_fetch.iter().enumerate() {
        match fetch_cp(&client, cp) {
            Ok(Some(g)) => {
                eprintln!("[{}/{}] {cp} → {}", i + 1, to_fetch.len(), g.commune);
                cache.insert((*cp).clone(), g);
            }
            Ok(None) => eprintln!("[{}/{}] {cp} → aucune commune trouvée", i + 1, to_fetch.len()),
            Err(e) => eprintln!("[{}/{}] {cp} → erreur: {e}", i + 1, to_fetch.len()),
        }
        std::thread::sleep(Duration::from_millis(args.delay_ms));
    }

    state::write_json_pretty(&args.cache, &cache, "cache")?;

    let mut points: Vec<Point> = Vec::new();
    let mut unmapped = 0u32;
    for (cp, indicatifs) in groups {
        if let Some(g) = cache.get(&cp) {
            points.push(Point {
                cp,
                commune: g.commune.clone(),
                lat: g.lat,
                lon: g.lon,
                indicatifs,
            });
        } else {
            unmapped += 1;
        }
    }
    eprintln!("{} points cartographiés ({unmapped} CP non géocodés)", points.len());

    let json = serde_json::to_string(&points)?;
    let html = HTML_TEMPLATE.replace(PLACEHOLDER, &json);
    std::fs::write(&args.output, html)
        .with_context(|| format!("écriture {}", args.output.display()))?;
    eprintln!("→ {}", args.output.display());
    Ok(())
}
