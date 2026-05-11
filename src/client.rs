use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::cookie::Jar;
use reqwest::header::{self, HeaderMap, HeaderValue};

pub const USER_AGENT: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:150.0) Gecko/20100101 Firefox/150.0";
pub const ORIGIN: &str = "https://annuaire-amateurs.anfr.fr";

fn default_headers() -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert(header::USER_AGENT, HeaderValue::from_static(USER_AGENT));
    h
}

pub fn build(jar: Option<Arc<Jar>>) -> Result<Client> {
    let mut b = Client::builder()
        .http1_only()
        .http1_title_case_headers()
        .pool_max_idle_per_host(0)
        .referer(false)
        .default_headers(default_headers())
        .timeout(Duration::from_secs(30));
    if let Some(j) = jar {
        b = b.cookie_provider(j);
    }
    Ok(b.build()?)
}
