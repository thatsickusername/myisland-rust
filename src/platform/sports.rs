use crossbeam_channel::Sender;
use std::{collections::HashMap, io::Read, time::Duration};

use crate::state::SportsState;
use serde_json::Value;

pub struct SportsService;

struct ParsedSports {
    state: SportsState,
    home_logo_url: Option<String>,
    away_logo_url: Option<String>,
}

impl SportsService {
    pub fn start(tx: Sender<SportsState>, endpoint: String) {
        std::thread::spawn(move || {
            let mut logo_cache = HashMap::<String, Vec<u8>>::new();
            loop {
                Self::poll(&tx, &endpoint, &mut logo_cache);
                std::thread::sleep(Duration::from_secs(15));
            }
        });
    }

    fn poll(tx: &Sender<SportsState>, endpoint: &str, logo_cache: &mut HashMap<String, Vec<u8>>) {
        if let Ok(res) = ureq::get(endpoint).set("User-Agent", "MyIsland/0.1").call() {
            if let Ok(json) = res.into_json::<Value>() {
                if let Some(parsed) = Self::parse_scoreboard(&json) {
                    let mut state = parsed.state;
                    state.home_logo = Self::load_logo(logo_cache, parsed.home_logo_url.as_deref());
                    state.away_logo = Self::load_logo(logo_cache, parsed.away_logo_url.as_deref());
                    let _ = tx.send(state);
                }
            }
        }
    }

    fn parse_scoreboard(json: &Value) -> Option<ParsedSports> {
        let events = json.get("events")?.as_array()?;

        // Prefer a live event, but keep a completed/upcoming event available so
        // the panel can show a useful score while the league is between matches.
        let event = events
            .iter()
            .find(|event| Self::competition_is_live(event))
            .or_else(|| events.first())?;
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
        let clock = if is_upcoming {
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
                    .format("%a, %b %-d · %-I:%M %p")
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
    use super::SportsService;
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

        let parsed = SportsService::parse_scoreboard(&scoreboard).expect("valid match");
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
        assert!(SportsService::parse_scoreboard(&json!({ "events": [] })).is_none());
        assert!(SportsService::parse_scoreboard(&json!({
            "events": [{ "competitions": [] }]
        }))
        .is_none());
    }
}
