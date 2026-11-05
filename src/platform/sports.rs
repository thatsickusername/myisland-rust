use crossbeam_channel::Sender;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use crate::state::SportsState;
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct SportsConfig {
    pub subscriptions: Vec<TeamSubscription>,
}

#[derive(Debug, Deserialize, Serialize, Clone, PartialEq)]
pub struct TeamSubscription {
    pub team_abbr: String,
    pub league_endpoint: String,
}

impl Default for SportsConfig {
    fn default() -> Self {
        Self {
            subscriptions: vec![TeamSubscription {
                team_abbr: "ARS".to_string(), // Default fallback
                league_endpoint: "https://site.api.espn.com/apis/site/v2/sports/soccer/eng.1/scoreboard".to_string(),
            }],
        }
    }
}

/// Messages published by the sports polling worker.
///
/// `Loading` is emitted as soon as the settings file changes (and on startup)
/// so the island can replace stale matches with a fetching state while the
/// worker repopulates its cache.
#[derive(Debug, Clone)]
pub enum SportsEvent {
    Loading,
    Loaded,
    Match(SportsState),
}

/// How often an endpoint should be revisited based on what it returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PollPhase {
    /// Request failed; retry calmly instead of hammering a broken endpoint.
    Error,
    /// No event for the subscribed team(s) today. Sleep until the next day.
    Dormant,
    /// Today's match is finished.
    Final,
    /// Match is scheduled in the future. Holds the UNIX timestamp of kickoff.
    Scheduled(i64),
    /// Match is in progress.
    Live,
}

impl PollPhase {
    fn interval(self) -> Duration {
        match self {
            // Aggressive only while the ball is actually in play.
            PollPhase::Live => Duration::from_secs(15),
            // Sleep exactly until the kickoff + 5 seconds to catch the state
            // transition instantly without hammering the API beforehand.
            PollPhase::Scheduled(kickoff_ts) => {
                let now = chrono::Utc::now().timestamp();
                if kickoff_ts > now {
                    let wait_secs = (kickoff_ts - now + 5) as u64;
                    Duration::from_secs(wait_secs).min(Duration::from_secs(12 * 60 * 60))
                } else {
                    // Fallback polling if we missed it or the API state lags.
                    Duration::from_secs(60)
                }
            }
            // Pre/post: no need to check more than every five minutes.
            PollPhase::Final => Duration::from_secs(5 * 60),
            // Nothing today: resolve the active competition again tomorrow.
            PollPhase::Dormant => Duration::from_secs(12 * 60 * 60),
            PollPhase::Error => Duration::from_secs(5 * 60),
        }
    }

    fn rank(self) -> u8 {
        match self {
            PollPhase::Error => 0,
            PollPhase::Dormant => 1,
            PollPhase::Final => 2,
            PollPhase::Scheduled(_) => 4,
            PollPhase::Live => 5,
        }
    }
}

pub struct SportsService;

struct ParsedSports {
    state: SportsState,
    home_logo_url: Option<String>,
    away_logo_url: Option<String>,
}

impl SportsService {
    pub fn start(tx: Sender<SportsEvent>) {
        std::thread::spawn(move || {
            let mut logo_cache = HashMap::<String, Vec<u8>>::new();
            let config_dir = std::env::var("APPDATA")
                .map(|p| PathBuf::from(p).join("MyIsland"))
                .unwrap_or_else(|_| PathBuf::from("."));
            std::fs::create_dir_all(&config_dir).ok();
            let config_path = config_dir.join("sports_settings.json");

            let mut loaded_config: Option<SportsConfig> = None;
            let mut last_stamp: Option<(Option<SystemTime>, u64)> = None;
            let mut next_poll: HashMap<String, Instant> = HashMap::new();
            let mut force_refresh = true;

            loop {
                // ---------------------------------------------------------
                // 1. Config watch (hot reload)
                // ---------------------------------------------------------
                // Only re-read when the file's timestamp or size changes;
                // this keeps the 1s tick cheap.
                let metadata = std::fs::metadata(&config_path).ok();
                let stamp = metadata.as_ref().and_then(|m| m.modified().ok());
                let len = metadata.as_ref().map_or(0, |m| m.len());
                let should_read = loaded_config.is_none()
                    || last_stamp.map_or(true, |(prev_stamp, prev_len)| {
                        prev_stamp != stamp || prev_len != len
                    });

                if should_read {
                    if let Some(config) = Self::load_config(&config_path) {
                        last_stamp = Some((stamp, len));
                        // Compare parsed content so a touched-but-unchanged
                        // file does not clear the polling cache.
                        if loaded_config.as_ref() != Some(&config) {
                            loaded_config = Some(config);
                            next_poll.clear();
                            force_refresh = true;
                            let _ = tx.send(SportsEvent::Loading);
                        }
                    }
                    // A partially written / invalid file leaves the stamp
                    // untouched so the next tick retries the read.
                }

                let Some(config) = loaded_config.as_ref() else {
                    std::thread::sleep(Duration::from_secs(1));
                    continue;
                };

                // Group subscriptions by endpoint. Multiple teams (and the
                // bulk soccer subscriptions) share scoreboards, so each URL
                // is fetched at most once per cycle.
                let mut endpoints: Vec<(&str, Vec<&str>)> = Vec::new();
                for sub in &config.subscriptions {
                    if let Some((_, abbrs)) = endpoints
                        .iter_mut()
                        .find(|(endpoint, _)| *endpoint == sub.league_endpoint.as_str())
                    {
                        if !abbrs.contains(&sub.team_abbr.as_str()) {
                            abbrs.push(&sub.team_abbr);
                        }
                    } else {
                        endpoints.push((&sub.league_endpoint, vec![&sub.team_abbr]));
                    }
                }

                // ---------------------------------------------------------
                // 2. Immediate staggered refresh after a config change
                // ---------------------------------------------------------
                if force_refresh {
                    for (index, (endpoint, abbrs)) in endpoints.iter().enumerate() {
                        // Spread the burst so ESPN's edge does not see a
                        // wall of requests from a single save.
                        if index > 0 {
                            std::thread::sleep(Duration::from_millis(500));
                        }
                        let phase = Self::poll_endpoint(&tx, endpoint, abbrs, &mut logo_cache);
                        next_poll.insert((*endpoint).to_string(), Instant::now() + phase.interval());
                    }
                    force_refresh = false;
                    let _ = tx.send(SportsEvent::Loaded);
                }

                // ---------------------------------------------------------
                // 3. Adaptive scheduling
                // ---------------------------------------------------------
                let now = Instant::now();
                let mut due: Option<(&str, &Vec<&str>)> = None;
                let mut earliest = now + Duration::from_secs(1);
                for (endpoint, abbrs) in &endpoints {
                    match next_poll.get(*endpoint) {
                        Some(at) if *at <= now => {
                            due = Some((*endpoint, abbrs));
                            break;
                        }
                        Some(at) if *at < earliest => earliest = *at,
                        None => {
                            due = Some((*endpoint, abbrs));
                            break;
                        }
                        _ => {}
                    }
                }

                if let Some((endpoint, abbrs)) = due {
                    let phase = Self::poll_endpoint(&tx, endpoint, abbrs, &mut logo_cache);
                    next_poll.insert(endpoint.to_string(), Instant::now() + phase.interval());
                    std::thread::sleep(Duration::from_millis(500));
                } else {
                    // Sleep in ~1s slices (bounded by the next due poll) so
                    // a settings save is noticed quickly.
                    let wait = earliest
                        .saturating_duration_since(now)
                        .min(Duration::from_secs(1));
                    std::thread::sleep(wait.max(Duration::from_millis(50)));
                }
            }
        });
    }

    /// Read and parse the settings file. A missing file is seeded with the
    /// default config; an invalid (e.g. mid-write) file returns `None` so the
    /// caller keeps its last good config and retries.
    fn load_config(path: &Path) -> Option<SportsConfig> {
        match std::fs::read_to_string(path) {
            Ok(data) => serde_json::from_str::<SportsConfig>(&data).ok(),
            Err(_) => {
                let default_cfg = SportsConfig::default();
                if let Ok(json) = serde_json::to_string_pretty(&default_cfg) {
                    let _ = std::fs::write(path, json);
                }
                Some(default_cfg)
            }
        }
    }

    /// Fetch one scoreboard once and publish a match update for every
    /// subscribed team that appears in it. Returns the phase used to schedule
    /// the next poll for this endpoint.
    fn poll_endpoint(
        tx: &Sender<SportsEvent>,
        endpoint: &str,
        target_abbrs: &[&str],
        logo_cache: &mut HashMap<String, Vec<u8>>,
    ) -> PollPhase {
        let response = match ureq::get(endpoint)
            .set("User-Agent", "MyIsland/0.1")
            .timeout(Duration::from_secs(10))
            .call()
        {
            Ok(response) => response,
            Err(_) => return PollPhase::Error,
        };

        let json = match response.into_json::<Value>() {
            Ok(json) => json,
            Err(_) => return PollPhase::Error,
        };

        for target_abbr in target_abbrs {
            if let Some(parsed) = Self::parse_scoreboard(&json, target_abbr) {
                let mut state = parsed.state;
                state.home_logo = Self::load_logo(logo_cache, parsed.home_logo_url.as_deref());
                state.away_logo = Self::load_logo(logo_cache, parsed.away_logo_url.as_deref());
                let _ = tx.send(SportsEvent::Match(state));
            }
        }

        Self::scoreboard_phase(&json, target_abbrs)
    }

    /// Classify a scoreboard into a polling phase using the best (most
    /// urgent) event that involves any of the subscribed teams.
    fn scoreboard_phase(json: &Value, target_abbrs: &[&str]) -> PollPhase {
        let Some(events) = json.get("events").and_then(Value::as_array) else {
            return PollPhase::Dormant;
        };

        let mut best = PollPhase::Dormant;
        for event in events {
            if !target_abbrs
                .iter()
                .any(|abbr| Self::event_has_team(event, abbr))
            {
                continue;
            }

            let state = event
                .get("competitions")
                .and_then(Value::as_array)
                .and_then(|competitions| competitions.first())
                .and_then(|competition| competition.get("status"))
                .and_then(|status| status.get("type"))
                .and_then(|status_type| status_type.get("state"))
                .and_then(Value::as_str)
                .unwrap_or("");

            let phase = match state {
                "in" => PollPhase::Live,
                "pre" => PollPhase::Scheduled(Self::kickoff_timestamp(event)),
                "post" => PollPhase::Final,
                _ => PollPhase::Scheduled(Self::kickoff_timestamp(event)),
            };

            if phase.rank() > best.rank() {
                best = phase;
            }
        }

        best
    }

    /// Resolve the exact RFC 3339 kickoff timestamp for an event. Missing or
    /// malformed dates fall back to a short re-check instead of a long sleep.
    fn kickoff_timestamp(event: &Value) -> i64 {
        event
            .get("date")
            .and_then(Value::as_str)
            .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
            .map(|kickoff| kickoff.with_timezone(&chrono::Utc).timestamp())
            .unwrap_or_else(|| chrono::Utc::now().timestamp() + 300)
    }

    fn parse_scoreboard(json: &Value, target_abbr: &str) -> Option<ParsedSports> {
        let events = json.get("events")?.as_array()?;

        // Find the subscribed team's event, preferring a live match so the
        // panel shows the most relevant score for that team.
        let event = events
            .iter()
            .filter(|event| Self::event_has_team(event, target_abbr))
            .find(|event| Self::competition_is_live(event))
            .or_else(|| {
                events
                    .iter()
                    .find(|event| Self::event_has_team(event, target_abbr))
            })?;
        let comp = event.get("competitions")?.as_array()?.first()?;
        let competitors = comp.get("competitors")?.as_array()?;

        let home = competitors
            .iter()
            .find(|team| team.get("homeAway").and_then(Value::as_str) == Some("home"))
            .or_else(|| competitors.first())?;
        let away = competitors
            .iter()
            .find(|team| team.get("homeAway").and_then(Value::as_str) == Some("away"))
            .or_else(|| competitors.get(1))?;

        let home_team = Self::team_name(home)?;
        let away_team = Self::team_name(away)?;
        let status = comp.get("status").unwrap_or(&Value::Null);
        let status_type = status.get("type").unwrap_or(&Value::Null);
        let state = status_type
            .get("state")
            .and_then(Value::as_str)
            .unwrap_or("");
        let is_live = state == "in";
        let is_upcoming = state == "pre";
        let raw_clock = if is_upcoming {
            Self::scheduled_label(event, status_type)
        } else {
            status
                .get("displayClock")
                .and_then(Value::as_str)
                .or_else(|| status_type.get("shortDetail").and_then(Value::as_str))
                .or_else(|| status_type.get("detail").and_then(Value::as_str))
                .unwrap_or("")
                .to_string()
        };

        // Sanitize stray ESPN Unicode blocks (blinking squares, red cards,
        // emoji markers) that the UI font cannot rasterize. Keep the ASCII
        // range plus the middle dot used by the scheduled label.
        let clock = raw_clock
            .chars()
            .filter(|&c| c.is_ascii() || c == '\u{00B7}')
            .collect::<String>()
            .trim()
            .to_string();

        Some(ParsedSports {
            state: SportsState {
                home_team,
                home_score: Self::score(home),
                home_color: Self::parse_hex_color(
                    home.get("team")
                        .and_then(|team| team.get("color"))
                        .and_then(Value::as_str)
                        .unwrap_or("000000"),
                ),
                home_logo: None,
                away_team,
                away_score: Self::score(away),
                away_color: Self::parse_hex_color(
                    away.get("team")
                        .and_then(|team| team.get("color"))
                        .and_then(Value::as_str)
                        .unwrap_or("ffffff"),
                ),
                away_logo: None,
                clock,
                is_live,
                is_upcoming,
                latest_event: None,
            },
            home_logo_url: Self::logo_url(home),
            away_logo_url: Self::logo_url(away),
        })
    }

    fn scheduled_label(event: &Value, status_type: &Value) -> String {
        if let Some(date) = event.get("date").and_then(Value::as_str) {
            if let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(date) {
                return parsed
                    .with_timezone(&chrono::Local)
                    .format("%a, %b %-d \u{00B7} %-I:%M %p")
                    .to_string();
            }
        }

        status_type
            .get("detail")
            .and_then(Value::as_str)
            .or_else(|| status_type.get("shortDetail").and_then(Value::as_str))
            .unwrap_or("Scheduled")
            .to_string()
    }

    fn event_has_team(event: &Value, target_abbr: &str) -> bool {
        event
            .get("competitions")
            .and_then(Value::as_array)
            .and_then(|comps| comps.first())
            .and_then(|c| c.get("competitors"))
            .and_then(Value::as_array)
            .map_or(false, |competitors| {
                competitors.iter().any(|team| {
                    team.get("team")
                        .and_then(|t| t.get("abbreviation"))
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .eq_ignore_ascii_case(target_abbr)
                })
            })
    }

    fn competition_is_live(event: &Value) -> bool {
        event
            .get("competitions")
            .and_then(Value::as_array)
            .and_then(|competitions| competitions.first())
            .and_then(|competition| competition.get("status"))
            .and_then(|status| status.get("type"))
            .and_then(|status_type| status_type.get("state"))
            .and_then(Value::as_str)
            == Some("in")
    }

    fn team_name(team: &Value) -> Option<String> {
        let details = team.get("team")?;
        details
            .get("abbreviation")
            .and_then(Value::as_str)
            .or_else(|| details.get("shortDisplayName").and_then(Value::as_str))
            .or_else(|| details.get("displayName").and_then(Value::as_str))
            .filter(|name| !name.is_empty())
            .map(str::to_string)
    }

    fn logo_url(team: &Value) -> Option<String> {
        team.get("team")?
            .get("logo")
            .and_then(Value::as_str)
            .filter(|url| !url.is_empty())
            .map(str::to_string)
    }

    fn load_logo(cache: &mut HashMap<String, Vec<u8>>, url: Option<&str>) -> Option<Vec<u8>> {
        let url = url?.trim();
        if url.is_empty() {
            return None;
        }
        if let Some(bytes) = cache.get(url) {
            return Some(bytes.clone());
        }

        let response = ureq::get(url)
            .set("User-Agent", "MyIsland/0.1")
            .timeout(Duration::from_secs(4))
            .call()
            .ok()?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .ok()?;
        if bytes.is_empty() || image::load_from_memory(&bytes).is_err() {
            return None;
        }

        cache.insert(url.to_string(), bytes.clone());
        Some(bytes)
    }

    fn score(team: &Value) -> u32 {
        team.get("score")
            .and_then(|score| {
                score
                    .as_u64()
                    .or_else(|| score.as_str().and_then(|value| value.parse().ok()))
            })
            .unwrap_or(0) as u32
    }

    fn parse_hex_color(hex: &str) -> [u8; 4] {
        let hex = hex.trim_start_matches('#');
        if hex.len() < 6 {
            return [255, 255, 255, 255];
        }

        let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(255);
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(255);
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(255);
        [r, g, b, 255]
    }
}

#[cfg(test)]
mod tests {
    use super::{PollPhase, SportsService};
    use serde_json::json;

    #[test]
    fn scheduled_match_uses_kickoff_and_preserves_team_order() {
        let scoreboard = json!({
            "events": [{
                "date": "2026-11-06T20:00:00Z",
                "competitions": [{
                    "competitors": [
                        {
                            "homeAway": "away",
                            "score": "0",
                            "team": {
                                "abbreviation": "COV",
                                "color": "87cced",
                                "logo": "https://example.test/cov.png"
                            }
                        },
                        {
                            "homeAway": "home",
                            "score": "0",
                            "team": {
                                "abbreviation": "EVE",
                                "color": "0606fa",
                                "logo": "https://example.test/eve.png"
                            }
                        }
                    ],
                    "status": {
                        "displayClock": "0'",
                        "type": {
                            "state": "pre",
                            "detail": "Fri, November 6th at 3:00 PM EST",
                            "shortDetail": "Scheduled"
                        }
                    }
                }]
            }]
        });

        let parsed = SportsService::parse_scoreboard(&scoreboard, "EVE").expect("valid match");
        assert_eq!(parsed.state.home_team, "EVE");
        assert_eq!(parsed.state.away_team, "COV");
        assert!(parsed.state.is_upcoming);
        assert!(!parsed.state.is_live);
        assert_ne!(parsed.state.clock, "0'");
        assert!(parsed.home_logo_url.is_some());
        assert!(parsed.away_logo_url.is_some());
    }

    #[test]
    fn empty_or_incomplete_scoreboards_are_ignored() {
        assert!(SportsService::parse_scoreboard(&json!({ "events": [] }), "ARS").is_none());
        assert!(SportsService::parse_scoreboard(
            &json!({
                "events": [{ "competitions": [] }]
            }),
            "ARS"
        )
        .is_none());
    }

    #[test]
    fn only_subscribed_team_matches_are_selected() {
        let scoreboard = json!({
            "events": [
                {
                    "competitions": [{
                        "competitors": [
                            { "homeAway": "home", "team": { "abbreviation": "LIV" } },
                            { "homeAway": "away", "team": { "abbreviation": "MCI" } }
                        ],
                        "status": { "type": { "state": "in" } }
                    }]
                },
                {
                    "competitions": [{
                        "competitors": [
                            { "homeAway": "home", "team": { "abbreviation": "ARS" } },
                            { "homeAway": "away", "team": { "abbreviation": "CHE" } }
                        ],
                        "status": { "type": { "state": "pre" } }
                    }]
                }
            ]
        });

        let parsed = SportsService::parse_scoreboard(&scoreboard, "ARS").expect("subscribed match");
        assert_eq!(parsed.state.home_team, "ARS");
        assert_eq!(parsed.state.away_team, "CHE");
    }

    #[test]
    fn scoreboard_phase_tracks_match_state() {
        let live = json!({
            "events": [{
                "competitions": [{
                    "competitors": [
                        { "team": { "abbreviation": "ARS" } },
                        { "team": { "abbreviation": "CHE" } }
                    ],
                    "status": { "type": { "state": "in" } }
                }]
            }]
        });
        assert_eq!(
            SportsService::scoreboard_phase(&live, &["ARS"]),
            PollPhase::Live
        );

        let final_score = json!({
            "events": [{
                "competitions": [{
                    "competitors": [
                        { "team": { "abbreviation": "ARS" } },
                        { "team": { "abbreviation": "CHE" } }
                    ],
                    "status": { "type": { "state": "post" } }
                }]
            }]
        });
        assert_eq!(
            SportsService::scoreboard_phase(&final_score, &["ARS"]),
            PollPhase::Final
        );

        // Empty scoreboard means the team is not playing today -> sleep.
        assert_eq!(
            SportsService::scoreboard_phase(&json!({ "events": [] }), &["ARS"]),
            PollPhase::Dormant
        );
    }

    #[test]
    fn live_phase_polls_fastest_and_dormant_phase_sleeps_longest() {
        assert_eq!(PollPhase::Live.interval().as_secs(), 15);
        let future = chrono::Utc::now().timestamp() + 3600;
        let scheduled = PollPhase::Scheduled(future);
        assert!(scheduled.interval().as_secs() > 3000);
        assert_eq!(PollPhase::Final.interval().as_secs(), 300);
        assert_eq!(PollPhase::Dormant.interval().as_secs(), 12 * 60 * 60);
    }
}
