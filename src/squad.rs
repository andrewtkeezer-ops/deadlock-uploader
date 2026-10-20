//! Soul Scribe fork: after a match is uploaded to deadlock-api, tell the squad site so the
//! leaderboard refreshes right away instead of waiting for its hourly sync.
//!
//! Fire-and-forget on a background thread; failures are logged at debug level and never
//! affect ingestion. Set `DEADLOCK_SQUAD_URL=` (empty) to disable, or point it elsewhere.

use std::sync::OnceLock;
use std::thread;

use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::utils::{Salts, http_client};

const DEFAULT_URL: &str = "https://deadlock.andrewtkeezer.workers.dev/api/ingest/notify";

fn url() -> &'static str {
    static URL: OnceLock<String> = OnceLock::new();
    URL.get_or_init(|| {
        std::env::var("DEADLOCK_SQUAD_URL").unwrap_or_else(|_| DEFAULT_URL.to_owned())
    })
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

// ---- In-game build sync -------------------------------------------------------------
// The squad site holds builds the author flagged "send to my in-game builds". The GC pass
// (game closed, logged in as the author) publishes them through `ClientToGCUpdateHeroBuild`
// and reports the resulting build id/version back so the site can show it.

/// Site origin, derived from the notify URL (so one env var moves everything).
fn site() -> Option<String> {
    let u = url();
    if u.is_empty() {
        return None;
    }
    Some(
        u.trim_end_matches("/api/ingest/notify")
            .trim_end_matches('/')
            .to_owned(),
    )
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct PendingMod {
    pub(crate) ability_id: u32,
    #[serde(default)]
    pub(crate) annotation: String,
    #[serde(default)]
    pub(crate) imbue_target_ability_id: u32,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct PendingCategory {
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) mods: Vec<PendingMod>,
}

#[derive(Deserialize, Debug, Clone)]
pub(crate) struct PendingBuild {
    pub(crate) id: u64,
    pub(crate) hero_build_id: Option<u32>,
    pub(crate) version: Option<u32>,
    pub(crate) hero_id: u32,
    #[serde(default)]
    pub(crate) name: String,
    #[serde(default)]
    pub(crate) description: String,
    #[serde(default)]
    pub(crate) categories: Vec<PendingCategory>,
}

/// Builds waiting to be published for `account_id`. Empty when the site is disabled or unreachable.
pub(crate) fn pending_builds(account_id: u32) -> Vec<PendingBuild> {
    let Some(site) = site() else {
        return Vec::new();
    };
    let target = format!("{site}/api/sync/builds?account_id={account_id}");
    match http_client().get(&target).call() {
        Ok(mut res) => res
            .body_mut()
            .read_json::<Vec<PendingBuild>>()
            .unwrap_or_else(|e| {
                debug!("squad builds: bad response: {e}");
                Vec::new()
            }),
        Err(e) => {
            debug!("squad builds: fetch failed: {e}");
            Vec::new()
        }
    }
}

#[derive(Serialize)]
struct BuildResult<'a> {
    account_id: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    hero_build_id: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<&'a str>,
}

/// Tell the site how publishing went (new id + version, or an error message).
pub(crate) fn report_build(account_id: u32, build_id: u64, outcome: Result<(u32, u32), &str>) {
    let Some(site) = site() else { return };
    let target = format!("{site}/api/sync/builds/{build_id}/result");
    let body = match outcome {
        Ok((hero_build_id, version)) => BuildResult {
            account_id,
            hero_build_id: Some(hero_build_id),
            version: Some(version),
            error: None,
        },
        Err(e) => BuildResult {
            account_id,
            hero_build_id: None,
            version: None,
            error: Some(e),
        },
    };
    if let Err(e) = http_client().post(&target).send_json(&body) {
        debug!("squad build result failed: {e}");
    }
}
