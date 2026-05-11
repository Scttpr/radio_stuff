use anyhow::Result;
use reqwest::blocking::Client;
use reqwest::header;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::client::ORIGIN;
use crate::html_parse::{self, Record};
use crate::indicatif::Indicatif;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Session {
    pub p_instance: String,
    pub p_request: String,
    pub x01: String,
    pub x02: String,
    pub protected: String,
    pub salt: String,
    pub cookie: String,
}

#[derive(Debug)]
pub struct WafRejected;

impl std::fmt::Display for WafRejected {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "WAF a rejeté la requête")
    }
}

impl std::error::Error for WafRejected {}

#[derive(Debug)]
pub struct SessionExpired;

impl std::fmt::Display for SessionExpired {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "session APEX expirée")
    }
}

impl std::error::Error for SessionExpired {}

pub fn search(client: &Client, session: &Session, indicatif: &Indicatif) -> Result<Vec<Record>> {
    let indicatif = indicatif.as_str();
    let url = format!(
        "{ORIGIN}/ords/wwv_flow.ajax?p_context=2003:312:{}",
        session.p_instance
    );
    let referer = format!("{ORIGIN}/ords/f?p=2003:312:{}::NO:RP::", session.p_instance);

    let p_json = json!({
        "pageItems": {
            "itemsToSubmit": [
                {"n": "P312_INDICATIF", "v": indicatif},
                {"n": "P312_NOM", "v": ""},
                {"n": "P312_CP", "v": ""},
            ],
            "protected": session.protected,
            "rowVersion": "",
            "formRegionChecksums": [],
        },
        "salt": session.salt,
    });
    let p_json_str = serde_json::to_string(&p_json)?;

    let form: [(&str, &str); 12] = [
        ("p_flow_id", "2003"),
        ("p_flow_step_id", "312"),
        ("p_instance", &session.p_instance),
        ("p_debug", ""),
        ("p_request", &session.p_request),
        ("p_widget_name", "worksheet"),
        ("p_widget_mod", "ACTION"),
        ("p_widget_action", "QUICK_FILTER"),
        ("p_widget_num_return", "50"),
        ("x01", &session.x01),
        ("x02", &session.x02),
        ("p_json", &p_json_str),
    ];

    let response = client
        .post(&url)
        .header(header::ACCEPT, "text/html, */*; q=0.01")
        .header(
            header::CONTENT_TYPE,
            "application/x-www-form-urlencoded; charset=UTF-8",
        )
        .header("X-Requested-With", "XMLHttpRequest")
        .header(header::ORIGIN, ORIGIN)
        .header(header::REFERER, referer)
        .header(header::COOKIE, &session.cookie)
        .form(&form)
        .send()?
        .error_for_status()?;

    let html = response.text()?;

    if html_parse::is_waf_block(&html) {
        return Err(WafRejected.into());
    }
    if html_parse::is_session_expired(&html) {
        return Err(SessionExpired.into());
    }

    Ok(html_parse::parse_rows(&html))
}
