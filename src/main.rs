use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::Utc;
use clap::Parser;

use anfr_scrape::{client, html_parse, indicatif, renew, search, state};
use html_parse::Record;
use indicatif::Indicatif;
use search::{Session, SessionExpired, WafRejected};
use state::Vides;

const SAVE_BATCH: u32 = 100;
const MAX_CONSECUTIVE_RENEWS: u32 = 3;
const MAX_CONSECUTIVE_WAF: u32 = 5;

#[derive(Parser, Debug)]
#[command(about = "Scraper de l'annuaire ANFR amateur")]
struct Args {
    #[arg(long, default_value = "F4AAA", help = "indicatif de départ")]
    start: Indicatif,

    #[arg(long, default_value = "F4ZZZ", help = "indicatif de fin (inclus)")]
    end: Indicatif,

    #[arg(long, default_value_t = 1.0, help = "délai entre requêtes (s)")]
    delay: f64,

    #[arg(long = "max-retries", default_value_t = 6, help = "essais max par indicatif")]
    max_retries: u32,

    #[arg(long = "pause-every", default_value_t = 500, help = "pause longue tous les N indicatifs")]
    pause_every: u32,

    #[arg(long = "pause-duration", default_value_t = 60.0, help = "durée pause longue (s)")]
    pause_duration: f64,

    #[arg(long, default_value = "data/session.json", help = "fichier de session")]
    session: PathBuf,

    #[arg(long, default_value = "data/indicatifs.json", help = "fichier de sortie")]
    output: PathBuf,

    #[arg(long, default_value = "data/indicatifs_vides.json", help = "fichier des indicatifs vides")]
    vides: PathBuf,

    #[arg(long, help = "renouvelle la session.json avant de scraper")]
    renew: bool,

    #[arg(
        long = "waf-cooldown",
        default_value_t = 0.0,
        help = "attente (s) après rejet WAF avant de réessayer; 0 = arrêt immédiat"
    )]
    waf_cooldown: f64,

    #[arg(
        long = "update-validate",
        help = "ré-interroge les records réels existants (validation)"
    )]
    update_validate: bool,

    #[arg(
        long = "update-discover",
        help = "ré-interroge les indicatifs vides connus (redécouverte)"
    )]
    update_discover: bool,

    #[arg(
        long = "older-than",
        help = "en mode update, ne traite que les entrées non vérifiées depuis N jours"
    )]
    older_than: Option<i64>,
}

enum Attempt {
    Done(Vec<Record>),
    NeedsRenew,
    WafBlocked,
    Giveup,
}

fn try_with_backoff(
    max_retries: u32,
    indicatif: &Indicatif,
    mut f: impl FnMut() -> Result<Vec<Record>>,
) -> Attempt {
    let mut attempt = 0u32;
    loop {
        match f() {
            Ok(v) => return Attempt::Done(v),
            Err(e) => {
                if e.downcast_ref::<WafRejected>().is_some() {
                    return Attempt::WafBlocked;
                }
                if e.downcast_ref::<SessionExpired>().is_some() {
                    eprintln!("{indicatif}: session expirée, renouvellement...");
                    return Attempt::NeedsRenew;
                }
                attempt += 1;
                if attempt > max_retries {
                    eprintln!("{indicatif}: abandon après {} essais ({e})", attempt - 1);
                    return Attempt::Giveup;
                }
                let backoff = (1u64 << attempt).min(120);
                eprintln!("{indicatif}: erreur {e} (essai {attempt}, attente {backoff}s)");
                std::thread::sleep(Duration::from_secs(backoff));
            }
        }
    }
}

fn waf_abort_message(indicatif: &Indicatif) -> String {
    format!(
        "{indicatif}: WAF détecté\n\nConseils :\n  1. Ouvre https://annuaire-amateurs.anfr.fr/ords/f?p=2003:312 dans un onglet privé\n  2. Si la page charge → c'est notre profil de requête, pas l'IP\n  3. Si elle est rejetée → IP en cool-down, attends 30+ min\n  4. Reprend avec --start {indicatif} après cool-down\n  5. Ou utilise --waf-cooldown <secondes> pour attendre et réessayer automatiquement"
    )
}

/// Pilote une session ANFR : gère retry/backoff, renouvellement automatique
/// quand APEX expire, et cooldown WAF.
struct Scraper<'a> {
    client: &'a reqwest::blocking::Client,
    session: &'a mut Session,
    session_path: &'a Path,
    max_retries: u32,
    waf_cooldown: f64,
    consecutive_renews: u32,
    consecutive_waf: u32,
}

impl<'a> Scraper<'a> {
    fn new(
        client: &'a reqwest::blocking::Client,
        session: &'a mut Session,
        session_path: &'a Path,
        max_retries: u32,
        waf_cooldown: f64,
    ) -> Self {
        Self {
            client,
            session,
            session_path,
            max_retries,
            waf_cooldown,
            consecutive_renews: 0,
            consecutive_waf: 0,
        }
    }

    /// Interroge un indicatif. Retourne `Some(rows)` (succès, éventuellement vide),
    /// `None` si on abandonne après retries, ou propage une erreur fatale.
    fn query(&mut self, indicatif: &Indicatif) -> Result<Option<Vec<Record>>> {
        loop {
            let attempt = try_with_backoff(self.max_retries, indicatif, || {
                search::search(self.client, self.session, indicatif)
            });
            match attempt {
                Attempt::Done(rows) => {
                    self.consecutive_renews = 0;
                    self.consecutive_waf = 0;
                    return Ok(Some(rows));
                }
                Attempt::Giveup => return Ok(None),
                Attempt::NeedsRenew => self.handle_renew(indicatif)?,
                Attempt::WafBlocked => self.handle_waf(indicatif)?,
            }
        }
    }

    fn handle_renew(&mut self, indicatif: &Indicatif) -> Result<()> {
        self.consecutive_renews += 1;
        if self.consecutive_renews > MAX_CONSECUTIVE_RENEWS {
            anyhow::bail!(
                "{indicatif}: {MAX_CONSECUTIVE_RENEWS} renouvellements consécutifs ont échoué, arrêt"
            );
        }
        let fresh = renew::renew(self.session).context("renew session")?;
        state::save_session(self.session_path, &fresh)?;
        eprintln!("session renouvelée (p_instance={})", fresh.p_instance);
        *self.session = fresh;
        Ok(())
    }

    fn handle_waf(&mut self, indicatif: &Indicatif) -> Result<()> {
        if self.waf_cooldown <= 0.0 {
            anyhow::bail!("{}", waf_abort_message(indicatif));
        }
        self.consecutive_waf += 1;
        if self.consecutive_waf > MAX_CONSECUTIVE_WAF {
            anyhow::bail!(
                "{}",
                waf_abort_message(indicatif)
                    + &format!("\n\n  ({MAX_CONSECUTIVE_WAF} cooldowns WAF consécutifs sans succès)")
            );
        }
        eprintln!(
            "{indicatif}: WAF rejet ({}/{MAX_CONSECUTIVE_WAF}), attente {}s...",
            self.consecutive_waf, self.waf_cooldown,
        );
        std::thread::sleep(Duration::from_secs_f64(self.waf_cooldown));
        Ok(())
    }
}

struct Pacer {
    delay: f64,
    pause_every: u32,
    pause_duration: f64,
    processed: u32,
}

impl Pacer {
    fn new(delay: f64, pause_every: u32, pause_duration: f64) -> Self {
        Self {
            delay,
            pause_every,
            pause_duration,
            processed: 0,
        }
    }

    fn tick(&mut self) {
        std::thread::sleep(Duration::from_secs_f64(self.delay));
        self.processed += 1;
        if self.pause_every > 0 && self.processed.is_multiple_of(self.pause_every) {
            eprintln!(
                "-- pause {}s après {} indicatifs --",
                self.pause_duration, self.processed
            );
            std::thread::sleep(Duration::from_secs_f64(self.pause_duration));
        }
    }
}

/// Persistence batchée : accumule les modifications et flush tous les `batch`
/// changements (et au flush final). Réduit l'I/O en /N pour le même travail.
struct Persister<'a> {
    records_path: &'a Path,
    vides_path: &'a Path,
    batch: u32,
    dirty_records: u32,
    dirty_vides: u32,
}

impl<'a> Persister<'a> {
    fn new(records_path: &'a Path, vides_path: &'a Path, batch: u32) -> Self {
        Self {
            records_path,
            vides_path,
            batch,
            dirty_records: 0,
            dirty_vides: 0,
        }
    }

    fn touch_records(&mut self, records: &[Record]) -> Result<()> {
        self.dirty_records += 1;
        if self.dirty_records >= self.batch {
            state::save_records(self.records_path, records)?;
            self.dirty_records = 0;
        }
        Ok(())
    }

    fn touch_vides(&mut self, vides: &Vides) -> Result<()> {
        self.dirty_vides += 1;
        if self.dirty_vides >= self.batch {
            state::save_vides(self.vides_path, vides)?;
            self.dirty_vides = 0;
        }
        Ok(())
    }

    fn flush(&mut self, records: &[Record], vides: &Vides) -> Result<()> {
        if self.dirty_records > 0 {
            state::save_records(self.records_path, records)?;
            self.dirty_records = 0;
        }
        if self.dirty_vides > 0 {
            state::save_vides(self.vides_path, vides)?;
            self.dirty_vides = 0;
        }
        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.update_validate && args.update_discover {
        anyhow::bail!("--update-validate et --update-discover sont exclusifs");
    }

    let client = client::build(None)?;

    let mut session = if args.renew {
        let base = state::load_session(&args.session)?;
        eprintln!("renouvellement de la session...");
        let fresh = renew::renew(&base).context("renew session")?;
        state::save_session(&args.session, &fresh)?;
        eprintln!("session renouvelée (p_instance={})", fresh.p_instance);
        fresh
    } else {
        state::load_session(&args.session)?
    };

    let mut records = state::load_records(&args.output)?;
    let mut vides = state::load_vides(&args.vides)?;

    let mut scraper = Scraper::new(
        &client,
        &mut session,
        &args.session,
        args.max_retries,
        args.waf_cooldown,
    );
    let mut pacer = Pacer::new(args.delay, args.pause_every, args.pause_duration);
    let mut persister = Persister::new(&args.output, &args.vides, SAVE_BATCH);

    let result = if args.update_validate {
        run_validate(
            &mut scraper,
            &mut pacer,
            &mut persister,
            &args,
            &mut records,
            &mut vides,
        )
    } else if args.update_discover {
        run_discover(
            &mut scraper,
            &mut pacer,
            &mut persister,
            &args,
            &mut records,
            &mut vides,
        )
    } else {
        run_scrape(
            &mut scraper,
            &mut pacer,
            &mut persister,
            &args,
            &mut records,
            &mut vides,
        )
    };

    let flush = persister.flush(&records, &vides);
    result.and(flush)
}

fn in_range(i: &Indicatif, start: &Indicatif, end: &Indicatif) -> bool {
    i >= start && i <= end
}

fn run_scrape(
    scraper: &mut Scraper<'_>,
    pacer: &mut Pacer,
    persister: &mut Persister<'_>,
    args: &Args,
    records: &mut Vec<Record>,
    vides: &mut Vides,
) -> Result<()> {
    let mut seen: HashSet<Indicatif> = records.iter().map(|r| r.indicatif.clone()).collect();
    seen.extend(vides.keys().cloned());

    for current in Indicatif::range(args.start.clone(), args.end.clone()) {
        if seen.contains(&current) {
            continue;
        }

        let Some(rows) = scraper.query(&current)? else {
            continue;
        };

        if rows.is_empty() {
            println!("{current}: -");
            seen.insert(current.clone());
            vides.insert(current, Utc::now());
            persister.touch_vides(vides)?;
        } else {
            for r in rows {
                println!("{r:?}");
                if seen.insert(r.indicatif.clone()) {
                    records.push(r);
                }
            }
            persister.touch_records(records)?;
        }

        pacer.tick();
    }
    Ok(())
}

fn run_validate(
    scraper: &mut Scraper<'_>,
    pacer: &mut Pacer,
    persister: &mut Persister<'_>,
    args: &Args,
    records: &mut Vec<Record>,
    vides: &mut Vides,
) -> Result<()> {
    let cutoff = args
        .older_than
        .map(|d| Utc::now() - chrono::Duration::days(d));

    let targets: Vec<Indicatif> = records
        .iter()
        .filter(|r| in_range(&r.indicatif, &args.start, &args.end))
        .filter(|r| cutoff.is_none_or(|c| r.last_checked < c))
        .map(|r| r.indicatif.clone())
        .collect();

    eprintln!("validation : {} records à ré-interroger", targets.len());

    let mut updated = 0u32;
    let mut removed = 0u32;

    for current in targets {
        let Some(rows) = scraper.query(&current)? else {
            continue;
        };

        if rows.is_empty() {
            println!("{current}: disparu");
            records.retain(|x| x.indicatif != current);
            vides.insert(current.clone(), Utc::now());
            persister.touch_records(records)?;
            persister.touch_vides(vides)?;
            removed += 1;
            pacer.tick();
            continue;
        }

        let mut matched = false;
        for r in rows {
            if r.indicatif == current {
                matched = true;
            }
            if let Some(slot) = records.iter_mut().find(|x| x.indicatif == r.indicatif) {
                *slot = r;
            } else if !vides.contains_key(&r.indicatif) {
                records.push(r);
            }
        }

        if matched {
            updated += 1;
        } else {
            eprintln!("{current}: réponse sans correspondance, marqué vide");
            records.retain(|x| x.indicatif != current);
            vides.insert(current.clone(), Utc::now());
            persister.touch_vides(vides)?;
            removed += 1;
        }
        persister.touch_records(records)?;
        pacer.tick();
    }

    eprintln!("validation terminée : {updated} mis à jour, {removed} disparus");
    Ok(())
}

fn run_discover(
    scraper: &mut Scraper<'_>,
    pacer: &mut Pacer,
    persister: &mut Persister<'_>,
    args: &Args,
    records: &mut Vec<Record>,
    vides: &mut Vides,
) -> Result<()> {
    let cutoff = args
        .older_than
        .map(|d| Utc::now() - chrono::Duration::days(d));

    let targets: Vec<Indicatif> = vides
        .iter()
        .filter(|(i, _)| in_range(i, &args.start, &args.end))
        .filter(|(_, t)| cutoff.is_none_or(|c| **t < c))
        .map(|(i, _)| i.clone())
        .collect();

    eprintln!("redécouverte : {} indicatifs vides à ré-interroger", targets.len());

    let mut found = 0u32;
    let mut still_empty = 0u32;

    for current in targets {
        let Some(rows) = scraper.query(&current)? else {
            continue;
        };

        if rows.is_empty() {
            vides.insert(current, Utc::now());
            persister.touch_vides(vides)?;
            still_empty += 1;
        } else {
            for r in rows {
                println!("{r:?}");
                vides.remove(&r.indicatif);
                if let Some(slot) = records.iter_mut().find(|x| x.indicatif == r.indicatif) {
                    *slot = r;
                } else {
                    records.push(r);
                }
            }
            persister.touch_records(records)?;
            persister.touch_vides(vides)?;
            found += 1;
        }

        pacer.tick();
    }

    eprintln!("redécouverte terminée : {found} nouveaux, {still_empty} toujours vides");
    Ok(())
}
