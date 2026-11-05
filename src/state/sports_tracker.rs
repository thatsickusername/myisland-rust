use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use crate::state::SportsState;

/// Ordered, de-duplicated collection of the matches currently being tracked.
///
/// Insertion order is preserved so a card never jumps to another position
/// while the user is scrolling. `sort_initial` is only used once per config
/// load, before the user starts interacting.
pub struct SportsMatchList {
    matches: Vec<SportsState>,
    last_seen: HashMap<String, Instant>,
}

impl SportsMatchList {
    pub fn new() -> Self {
        Self {
            matches: Vec::new(),
            last_seen: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.matches.clear();
        self.last_seen.clear();
    }

    pub fn len(&self) -> usize {
        self.matches.len()
    }

    pub fn matches(&self) -> &[SportsState] {
        &self.matches
    }

    /// Insert or update a match, keeping its existing position. Returns the
    /// previous state for the same fixture, which callers use to detect goals
    /// and kickoffs.
    pub fn upsert(&mut self, state: SportsState) -> Option<SportsState> {
        let key = match_key(&state);
        let now = Instant::now();

        if let Some(index) = self.matches.iter().position(|m| match_key(m) == key) {
            let previous = self.matches[index].clone();
            self.matches[index] = state;
            self.last_seen.insert(key, now);
            Some(previous)
        } else {
            self.matches.push(state);
            self.last_seen.insert(key, now);
            None
        }
    }

    /// Drop fixtures that have not appeared in any scoreboard for `max_age`.
    /// This also clears stale live flags once a match rolls off the feeds.
    pub fn prune(&mut self, max_age: Duration) {
        let now = Instant::now();
        self.matches.retain(|state| {
            self.last_seen
                .get(&match_key(state))
                .map_or(false, |seen| now.duration_since(*seen) <= max_age)
        });

        let live_keys: HashSet<String> = self.matches.iter().map(match_key).collect();
        self.last_seen.retain(|key, _| live_keys.contains(key));
    }

    /// Live matches first, then upcoming, then finals. Stable, so the polling
    /// order is preserved within each group.
    pub fn sort_initial(&mut self) {
        self.matches.sort_by_key(|state| {
            if state.is_live {
                0
            } else if state.is_upcoming {
                1
            } else {
                2
            }
        });
    }
}

pub fn match_key(state: &SportsState) -> String {
    format!("{}-{}", state.home_team, state.away_team)
}

#[cfg(test)]
mod tests {
    use super::{match_key, SportsMatchList};
    use crate::state::SportsState;
    use std::time::Duration;

    fn state(home: &str, away: &str, live: bool, upcoming: bool) -> SportsState {
        SportsState {
            home_team: home.to_string(),
            home_score: 0,
            home_color: [0, 0, 0, 255],
            home_logo: None,
            away_team: away.to_string(),
            away_score: 0,
            away_color: [255, 255, 255, 255],
            away_logo: None,
            clock: String::new(),
            is_live: live,
            is_upcoming: upcoming,
            latest_event: None,
        }
    }

    #[test]
    fn updates_keep_position_and_return_previous_state() {
        let mut list = SportsMatchList::new();
        assert!(list.upsert(state("ARS", "CHE", false, true)).is_none());
        assert!(list.upsert(state("LIV", "MCI", false, true)).is_none());

        let mut updated = state("ARS", "CHE", false, true);
        updated.home_score = 2;
        let previous = list.upsert(updated).expect("previous state");
        assert_eq!(previous.home_score, 0);

        assert_eq!(list.len(), 2);
        assert_eq!(match_key(&list.matches()[0]), "ARS-CHE");
        assert_eq!(list.matches()[0].home_score, 2);
    }

    #[test]
    fn initial_sort_prefers_live_then_upcoming_then_final() {
        let mut list = SportsMatchList::new();
        list.upsert(state("FIN", "ONE", false, false));
        list.upsert(state("UPC", "TWO", false, true));
        list.upsert(state("LIV", "THR", true, false));

        list.sort_initial();

        assert_eq!(match_key(&list.matches()[0]), "LIV-THR");
        assert_eq!(match_key(&list.matches()[1]), "UPC-TWO");
        assert_eq!(match_key(&list.matches()[2]), "FIN-ONE");
    }

    #[test]
    fn prune_removes_only_stale_matches() {
        let mut list = SportsMatchList::new();
        list.upsert(state("ARS", "CHE", false, true));
        list.upsert(state("LIV", "MCI", true, false));

        list.prune(Duration::from_secs(3600));
        assert_eq!(list.len(), 2, "fresh matches must survive");

        std::thread::sleep(Duration::from_millis(2));
        list.prune(Duration::ZERO);
        assert_eq!(list.len(), 0, "expired matches must be dropped");
    }
}
