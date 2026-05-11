use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use anyhow::{Context, Result, anyhow};
use reqwest::StatusCode;
use reqwest::cookie::{CookieStore, Jar};
use reqwest::header;
use scraper::{Html, Selector};
use url::Url;

static INPUT_SELECTOR: LazyLock<Selector> = LazyLock::new(|| Selector::parse("input").unwrap());

use crate::client::{self, ORIGIN};
use crate::html_parse;
use crate::search::Session;

fn debug_dump_path() -> PathBuf {
    std::env::temp_dir().join("anfr_renew_debug.html")
}

pub fn renew(base: &Session) -> Result<Session> {
    let url = format!("{ORIGIN}/ords/f?p=2003:312");
    let url_parsed: Url = url.parse()?;

    let jar = Arc::new(Jar::default());
    let http = client::build(Some(Arc::clone(&jar)))?;

    let response = http
        .get(&url)
        .header(header::ACCEPT, "text/html, */*; q=0.01")
        .send()?
        .error_for_status()?;

    let final_url = response.url().clone();
    let status = response.status();
    let html = response.text()?;

    if html_parse::is_waf_block(&html) {
        return Err(dump_and_describe("WAF a rejeté le renew", status, &final_url, &html));
    }

    let cookie = jar
        .cookies(&url_parsed)
        .or_else(|| jar.cookies(&final_url))
        .map(|hv| hv.to_str().unwrap_or("").to_string())
        .unwrap_or_default();

    if cookie.is_empty() {
        return Err(dump_and_describe(
            "aucun cookie capturé",
            status,
            &final_url,
            &html,
        ));
    }

    let p_instance = extract_p_instance(&final_url, &html)
        .context("p_instance introuvable (URL et HTML)")?;

    let doc = Html::parse_document(&html);
    let ajax_id = extract_ajax_identifier(&html).ok_or_else(|| {
        dump_and_describe("ajaxIdentifier introuvable", status, &final_url, &html)
    })?;
    let p_request = format!("PLUGIN={ajax_id}");

    let salt = find_input(&doc, "id", "pSalt").ok_or_else(|| {
        dump_and_describe("pSalt introuvable", status, &final_url, &html)
    })?;
    let protected = find_input(&doc, "id", "pPageItemsProtected").ok_or_else(|| {
        dump_and_describe("pPageItemsProtected introuvable", status, &final_url, &html)
    })?;
    let x01 = find_input(&doc, "id", "annuaire_report_worksheet_id")
        .unwrap_or_else(|| base.x01.clone());
    let x02 = find_input(&doc, "id", "annuaire_report_report_id")
        .unwrap_or_else(|| base.x02.clone());

    Ok(Session {
        p_instance,
        p_request,
        x01,
        x02,
        protected,
        salt,
        cookie,
    })
}

fn dump_and_describe(reason: &str, status: StatusCode, final_url: &Url, html: &str) -> anyhow::Error {
    let path = debug_dump_path();
    let _ = std::fs::write(&path, html);
    anyhow!(
        "{reason} (status={status}, final_url={final_url}, html dumpé dans {}, taille={})",
        path.display(),
        html.len()
    )
}

fn extract_p_instance(url: &Url, html: &str) -> Option<String> {
    if let Some((_, value)) = url.query_pairs().find(|(k, _)| k == "p")
        && let Some(instance) = value.split(':').nth(2).filter(|s| !s.is_empty())
    {
        return Some(instance.to_string());
    }
    let doc = Html::parse_document(html);
    find_input(&doc, "name", "p_instance")
}

fn find_input(doc: &Html, attr: &str, value: &str) -> Option<String> {
    doc.select(&INPUT_SELECTOR)
        .find(|e| e.value().attr(attr) == Some(value))
        .and_then(|e| e.value().attr("value"))
        .map(str::to_string)
}

fn extract_ajax_identifier(html: &str) -> Option<String> {
    let (_, after) = html.split_once("\"regionId\":\"annuaire_report\"")?;
    let (_, after) = after.split_once("\"ajaxIdentifier\":\"")?;
    let (raw, _) = after.split_once('"')?;
    serde_json::from_str::<String>(&format!("\"{raw}\"")).ok()
}
