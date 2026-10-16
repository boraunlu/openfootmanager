use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::event::{EventType, MatchEvent};
use crate::types::{Side, Zone};

// ---------------------------------------------------------------------------
// TeamStats — aggregate stats for one side
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TeamStats {
    pub goals: u8,
    pub shots: u16,
    pub shots_on_target: u16,
    pub shots_off_target: u16,
    pub shots_blocked: u16,
    pub passes_completed: u16,
    pub passes_intercepted: u16,
    pub tackles: u16,
    pub interceptions: u16,
    pub fouls: u16,
    pub corners: u16,
    pub free_kicks: u16,
    pub penalties: u16,
    pub yellow_cards: u8,
    pub red_cards: u8,
    pub possession_ticks: u32,
}

impl TeamStats {
    pub fn pass_accuracy(&self) -> f64 {
        let total = self.passes_completed as f64 + self.passes_intercepted as f64;
        if total == 0.0 {
            return 0.0;
        }
        self.passes_completed as f64 / total * 100.0
    }
}

// ---------------------------------------------------------------------------
// PlayerMatchStats — individual player performance
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlayerMatchStats {
    pub minutes_played: u8,
    pub goals: u8,
    pub assists: u8,
    pub shots: u8,
    pub shots_on_target: u8,
    pub passes_completed: u8,
    pub passes_attempted: u8,
    pub tackles_won: u8,
    pub interceptions: u8,
    pub fouls_committed: u8,
    pub yellow_cards: u8,
    pub red_cards: u8,
    /// Match rating 1.0–10.0, computed after the match; 0.0 means unrated.
    pub rating: f32,
}

// ---------------------------------------------------------------------------
// GoalSource — how a goal was created (distinct from event.rs GoalContext which tracks narrative)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum GoalSource {
    OpenPlay,
    Corner,
    FreeKick,
    Penalty,
}

// ---------------------------------------------------------------------------
// GoalDetail — enriched goal info for the report
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoalDetail {
    pub minute: u8,
    pub scorer_id: String,
    pub assist_id: Option<String>,
    pub goal_source: GoalSource,
    pub side: Side,
}

// ---------------------------------------------------------------------------
// MatchReport — the complete output of a simulated match
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatchReport {
    pub home_goals: u8,
    pub away_goals: u8,
    pub home_stats: TeamStats,
    pub away_stats: TeamStats,
    pub events: Vec<MatchEvent>,
    pub goals: Vec<GoalDetail>,
    pub player_stats: HashMap<String, PlayerMatchStats>,
    /// Possession percentage for the home team (0–100).
    pub home_possession: f64,
    /// Total simulated minutes (90 + stoppage).
    pub total_minutes: u8,
    /// Penalty-shootout score when the match went to one; `None` otherwise.
    /// Shootout kicks are never counted in `home_goals`/`away_goals`.
    #[serde(default)]
    pub home_penalties: Option<u8>,
    #[serde(default)]
    pub away_penalties: Option<u8>,
}

impl MatchReport {
    /// Build the report from the raw event log and possession counters.
    pub fn from_events(
        events: Vec<MatchEvent>,
        home_possession_ticks: u32,
        away_possession_ticks: u32,
        total_minutes: u8,
    ) -> Self {
        Self::from_events_with_players(
            events,
            home_possession_ticks,
            away_possession_ticks,
            total_minutes,
            Vec::new(),
        )
    }

    /// Build the report while also assigning minutes played for tracked players.
    pub fn from_events_with_players(
        events: Vec<MatchEvent>,
        home_possession_ticks: u32,
        away_possession_ticks: u32,
        total_minutes: u8,
        tracked_player_ids: Vec<String>,
    ) -> Self {
        let mut home_stats = TeamStats::default();
        let mut away_stats = TeamStats::default();
        let mut goals = Vec::new();
        let mut player_stats: HashMap<String, PlayerMatchStats> = HashMap::new();

        home_stats.possession_ticks = home_possession_ticks;
        away_stats.possession_ticks = away_possession_ticks;

        // State machine to determine goal source from preceding set-piece event.
        // Tracks (event_type, side) so a set piece earned by one team doesn't
        // accidentally attribute a goal scored by the other.
        let mut last_set_piece: Option<(EventType, Side)> = None;

        for event in &events {
            let stats = match event.side {
                Side::Home => &mut home_stats,
                Side::Away => &mut away_stats,
            };

            // Track set-piece window: reset on events that clear the opportunity
            match &event.event_type {
                EventType::Corner => last_set_piece = Some((EventType::Corner, event.side)),
                EventType::FreeKick => {
                    // Only dangerous free kicks count: the taking side must be in their attacking
                    // third (opponent's defensive third). A free kick in HomeDefense is only
                    // dangerous when Away is taking it, and vice-versa.
                    let dangerous_zone = match event.side {
                        Side::Home => Zone::AwayDefense,
                        Side::Away => Zone::HomeDefense,
                    };
                    if event.zone == dangerous_zone {
                        last_set_piece = Some((EventType::FreeKick, event.side));
                    }
                }
                // Defensive events clear the set-piece window
                EventType::ShotOffTarget
                | EventType::ShotBlocked
                | EventType::ShotSaved
                | EventType::PenaltyMiss
                | EventType::Clearance
                | EventType::Interception
                | EventType::PassIntercepted
                | EventType::GoalKick => last_set_piece = None,
                _ => {}
            }

            // Update player stats helper
            let pid = event.player_id.as_deref().unwrap_or("");

            match &event.event_type {
                EventType::Goal => {
                    stats.goals += 1;
                    stats.shots += 1;
                    stats.shots_on_target += 1;
                    let source = match last_set_piece.take() {
                        Some((EventType::Corner, sp_side)) if sp_side == event.side => {
                            GoalSource::Corner
                        }
                        Some((EventType::FreeKick, sp_side)) if sp_side == event.side => {
                            GoalSource::FreeKick
                        }
                        _ => GoalSource::OpenPlay,
                    };
                    goals.push(GoalDetail {
                        minute: event.minute,
                        scorer_id: pid.to_string(),
                        assist_id: event.secondary_player_id.clone(),
                        goal_source: source,
                        side: event.side,
                    });
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.goals += 1;
                        ps.shots += 1;
                        ps.shots_on_target += 1;
                    }
                    if let Some(ref assist_id) = event.secondary_player_id {
                        let ps = player_stats.entry(assist_id.clone()).or_default();
                        ps.assists += 1;
                    }
                }
                EventType::PenaltyGoal => {
                    stats.goals += 1;
                    stats.shots += 1;
                    stats.shots_on_target += 1;
                    stats.penalties += 1;
                    last_set_piece = None;
                    goals.push(GoalDetail {
                        minute: event.minute,
                        scorer_id: pid.to_string(),
                        assist_id: None,
                        goal_source: GoalSource::Penalty,
                        side: event.side,
                    });
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.goals += 1;
                        ps.shots += 1;
                        ps.shots_on_target += 1;
                    }
                }
                EventType::PenaltyMiss => {
                    stats.shots += 1;
                    stats.penalties += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.shots += 1;
                    }
                }
                EventType::ShotOnTarget | EventType::ShotSaved => {
                    stats.shots += 1;
                    stats.shots_on_target += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.shots += 1;
                        ps.shots_on_target += 1;
                    }
                }
                EventType::ShotOffTarget => {
                    stats.shots += 1;
                    stats.shots_off_target += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.shots += 1;
                    }
                }
                EventType::ShotBlocked => {
                    stats.shots += 1;
                    stats.shots_blocked += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.shots += 1;
                    }
                }
                EventType::PassCompleted => {
                    stats.passes_completed += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.passes_completed += 1;
                        ps.passes_attempted += 1;
                    }
                }
                EventType::PassIntercepted => {
                    stats.passes_intercepted += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.passes_attempted += 1;
                    }
                }
                EventType::Tackle => {
                    stats.tackles += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.tackles_won += 1;
                    }
                }
                EventType::Interception => {
                    stats.interceptions += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.interceptions += 1;
                    }
                }
                EventType::Foul => {
                    stats.fouls += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.fouls_committed += 1;
                    }
                }
                EventType::YellowCard | EventType::SecondYellow => {
                    stats.yellow_cards += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.yellow_cards += 1;
                    }
                }
                EventType::RedCard => {
                    stats.red_cards += 1;
                    if !pid.is_empty() {
                        let ps = player_stats.entry(pid.to_string()).or_default();
                        ps.red_cards += 1;
                    }
                }
                EventType::Corner => {
                    stats.corners += 1;
                }
                EventType::FreeKick => {
                    stats.free_kicks += 1;
                }
                EventType::PenaltyAwarded => {
                    stats.penalties += 1;
                }
                // Shootout kicks are intentionally excluded from goals,
                // GoalDetails, and player stats — the shootout is scored
                // separately via home_penalties/away_penalties.
                EventType::ShootoutGoal | EventType::ShootoutMiss => {}
                _ => {}
            }
        }

        populate_minutes_played(
            &events,
            total_minutes,
            &tracked_player_ids,
            &mut player_stats,
        );
        calculate_match_ratings(&events, &mut player_stats);

        let total_poss = home_possession_ticks + away_possession_ticks;
        let home_possession = if total_poss > 0 {
            home_possession_ticks as f64 / total_poss as f64 * 100.0
        } else {
            50.0
        };

        Self {
            home_goals: home_stats.goals,
            away_goals: away_stats.goals,
            home_stats,
            away_stats,
            events,
            goals,
            player_stats,
            home_possession,
            total_minutes,
            home_penalties: None,
            away_penalties: None,
        }
    }
}

/// Deterministic v1 assessment of recorded contributions. Zero is an unrated
/// sentinel, never a bad performance. No ability, reputation or random inputs.
fn calculate_match_ratings(events: &[MatchEvent], players: &mut HashMap<String, PlayerMatchStats>) {
    use crate::event::{DangerBand, EventDetail, SaveQuality};
    #[derive(Default)]
    struct Extra {
        carrying: f32,
        clearances: f32,
        saves: f32,
        misses: f32,
        penalties: f32,
        dismissal: f32,
    }
    let mut extras: HashMap<&str, Extra> = HashMap::new();
    for event in events {
        if let Some(id) = event.player_id.as_deref() {
            let extra = extras.entry(id).or_default();
            match event.event_type {
                EventType::Dribble => extra.carrying += 0.04,
                EventType::DribbleTackled => extra.carrying -= 0.05,
                EventType::Clearance => extra.clearances += 0.06,
                EventType::PenaltyGoal => extra.penalties += 0.35,
                EventType::PenaltyMiss => extra.misses += 0.65,
                EventType::SecondYellow => extra.dismissal += 1.2,
                EventType::ShotOffTarget | EventType::ShotBlocked => {
                    extra.misses += if matches!(
                        event.detail,
                        Some(EventDetail::Shot {
                            danger: DangerBand::BigChance
                        })
                    ) {
                        0.20
                    } else {
                        0.03
                    }
                }
                _ => {}
            }
        }
        // ShotSaved's primary player is the shooter. New events explicitly
        // identify the saving goalkeeper as secondary; never guess for old logs.
        if event.event_type == EventType::ShotSaved
            && let Some(keeper) = event.secondary_player_id.as_deref()
        {
            extras.entry(keeper).or_default().saves += match event.detail {
                Some(EventDetail::Save {
                    quality: SaveQuality::WorldClass,
                }) => 0.35,
                Some(EventDetail::Save {
                    quality: SaveQuality::Strong,
                }) => 0.25,
                _ => 0.18,
            };
        }
    }
    for (id, stats) in players {
        // A same-minute cameo has too little recorded playing time to rate.
        if stats.minutes_played == 0 {
            stats.rating = 0.0;
            continue;
        }
        let extra = extras.get(id.as_str());
        let failed_passes = stats
            .passes_attempted
            .saturating_sub(stats.passes_completed);
        let passing = (f32::from(stats.passes_completed) * 0.025 - f32::from(failed_passes) * 0.04)
            .clamp(-0.7, 0.6);
        let defending = (f32::from(stats.tackles_won) * 0.1
            + f32::from(stats.interceptions) * 0.12
            + extra.map_or(0.0, |e| e.clearances))
        .min(1.2);
        let discipline = (f32::from(stats.fouls_committed) * 0.05).min(0.4)
            + f32::from(stats.yellow_cards) * 0.2
            + f32::from(stats.red_cards) * 1.4
            + extra.map_or(0.0, |e| e.dismissal);
        let value = 6.3
            + f32::from(stats.goals)
            + f32::from(stats.assists) * 0.6
            + passing
            + defending
            + extra.map_or(0.0, |e| {
                e.carrying.clamp(-0.6, 0.4) + e.saves.min(2.2) - e.misses.min(1.5) - e.penalties
            })
            - discipline;
        stats.rating = (value.clamp(1.0, 10.0) * 10.0).round() / 10.0;
    }
}

fn populate_minutes_played(
    events: &[MatchEvent],
    total_minutes: u8,
    tracked_player_ids: &[String],
    player_stats: &mut HashMap<String, PlayerMatchStats>,
) {
    let mut minutes_by_player: HashMap<String, u8> = tracked_player_ids
        .iter()
        .cloned()
        .map(|player_id| (player_id, total_minutes))
        .collect();

    for event in events {
        match event.event_type {
            EventType::Substitution => {
                if let Some(ref player_off_id) = event.secondary_player_id {
                    minutes_by_player
                        .insert(player_off_id.clone(), event.minute.min(total_minutes));
                }
                if let Some(ref player_on_id) = event.player_id {
                    minutes_by_player.insert(
                        player_on_id.clone(),
                        total_minutes.saturating_sub(event.minute),
                    );
                }
            }
            EventType::RedCard | EventType::SecondYellow => {
                if let Some(ref player_id) = event.player_id {
                    let dismissed_at = event.minute.min(total_minutes);
                    minutes_by_player
                        .entry(player_id.clone())
                        .and_modify(|minutes| *minutes = (*minutes).min(dismissed_at))
                        .or_insert(dismissed_at);
                }
            }
            _ => {}
        }
    }

    for (player_id, minutes_played) in minutes_by_player {
        player_stats.entry(player_id).or_default().minutes_played = minutes_played;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ratings_reward_contributions_and_penalize_discipline_without_ability_input() {
        let events = vec![
            event(10, EventType::Goal, Side::Home, "scorer").with_secondary("creator"),
            event(20, EventType::Tackle, Side::Home, "defender"),
            event(30, EventType::Interception, Side::Home, "defender"),
            event(40, EventType::Clearance, Side::Home, "defender"),
            event(50, EventType::RedCard, Side::Home, "dismissed"),
        ];
        let ids = ["quiet", "scorer", "creator", "defender", "dismissed"]
            .map(String::from)
            .to_vec();
        let report = MatchReport::from_events_with_players(events, 50, 50, 90, ids);
        let rating = |id: &str| report.player_stats[id].rating;
        assert!((6.0..=7.0).contains(&rating("quiet")));
        for id in ["scorer", "creator", "defender"] {
            assert!(rating(id) > rating("quiet"));
        }
        assert!(rating("dismissed") < rating("quiet"));
    }

    #[test]
    fn ratings_credit_recorded_keeper_and_ignore_shootouts() {
        let ids = ["shooter", "keeper", "quiet"].map(String::from).to_vec();
        let save = event(10, EventType::ShotSaved, Side::Home, "shooter").with_secondary("keeper");
        let before =
            MatchReport::from_events_with_players(vec![save.clone()], 50, 50, 90, ids.clone());
        let after = MatchReport::from_events_with_players(
            vec![
                save,
                event(120, EventType::ShootoutGoal, Side::Home, "shooter"),
            ],
            50,
            50,
            90,
            ids,
        );
        assert!(before.player_stats["keeper"].rating > before.player_stats["quiet"].rating);
        assert!(before.player_stats["shooter"].rating <= before.player_stats["quiet"].rating);
        assert_eq!(
            before.player_stats["shooter"].rating,
            after.player_stats["shooter"].rating
        );
    }

    #[test]
    fn ratings_are_bounded_deterministic_and_zero_minutes_remain_unrated() {
        let events = (1..40)
            .map(|minute| event(minute, EventType::Goal, Side::Home, "scorer"))
            .collect::<Vec<_>>();
        let ids = vec!["scorer".into()];
        let a = MatchReport::from_events_with_players(events.clone(), 50, 50, 90, ids.clone());
        let b = MatchReport::from_events_with_players(events, 50, 50, 90, ids);
        assert!((1.0..=10.0).contains(&a.player_stats["scorer"].rating));
        assert_eq!(
            a.player_stats["scorer"].rating,
            b.player_stats["scorer"].rating
        );
        let empty = MatchReport::from_events_with_players(vec![], 0, 0, 0, vec!["unused".into()]);
        assert_eq!(empty.player_stats["unused"].rating, 0.0);
    }

    fn event(minute: u8, event_type: EventType, side: Side, player: &str) -> MatchEvent {
        MatchEvent::new(minute, event_type, side, Zone::attacking_box(side)).with_player(player)
    }

    // Regression: shootout kicks used to be counted as match goals, inflating
    // the scoreline (1-1 won 4-3 on pens was reported as 5-4) and the
    // scorers' goal tallies.
    #[test]
    fn shootout_kicks_are_not_goals() {
        let events = vec![
            event(20, EventType::Goal, Side::Home, "h1"),
            event(55, EventType::PenaltyGoal, Side::Away, "a1"),
            // Shootout after extra time
            event(121, EventType::ShootoutGoal, Side::Home, "h1"),
            event(121, EventType::ShootoutGoal, Side::Away, "a2"),
            event(122, EventType::ShootoutGoal, Side::Home, "h2"),
            event(122, EventType::ShootoutMiss, Side::Away, "a3"),
        ];
        let report = MatchReport::from_events(events, 50, 50, 120);

        assert_eq!(report.home_goals, 1);
        assert_eq!(report.away_goals, 1);
        assert_eq!(
            report.goals.len(),
            2,
            "GoalDetails must exclude shootout kicks"
        );
        assert_eq!(report.player_stats["h1"].goals, 1);
        assert_eq!(report.player_stats["a1"].goals, 1);
        assert!(
            report.player_stats.get("h2").is_none_or(|p| p.goals == 0),
            "shootout-only kicker must not be credited a goal"
        );
        // In-match penalties still count.
        assert_eq!(report.goals[1].goal_source, GoalSource::Penalty);
    }
}
