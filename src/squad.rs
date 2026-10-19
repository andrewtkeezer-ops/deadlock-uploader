//! Soul Scribe fork: after a match is uploaded to deadlock-api, tell the squad site so the
//! leaderboard refreshes right away instead of waiting for its hourly sync.
//!
//! Fire-and-forget on a background thread; failures are logged at debug level and never
//! affect ingestion. Set `DEADLOCK_SQUAD_URL=` (empty) to disable, or point it elsewhere.

use std::sync::OnceLock;
use std::thread;

use serde::Serialize;
use tracing::debug;

use crate::utils::{Salts, http_client};

const DEFAULT_URL: &str = "https://deadlock.andrewtkeezer.workers.dev/api/ingest/notify";

fn url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| std::env::var("DEADLOCK_SQUAD_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned()))
}

#[derive(Serialize)]
struct Notify {
    match_id: u64,
    account_id: Option<u32>,
}

pub(crate) fn notify(salt: &Salts) {
    let target = url();
    if target.is_empty() {
        return;
    }
    let body = Notify {
        match_id: salt.match_id,
        account_id: salt.username.or_else(crate::steam_user::current_steam_id3),
    };
    thread::spawn(move || {
        if let Err(e) = http_client().post(target).send_json(&body) {
            debug!("squad notify failed: {e}");
        }
    });
}
