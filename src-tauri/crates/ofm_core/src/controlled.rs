//! Explicit human ownership and isolated native manager contexts.
//! Context swaps are synchronous, candidate-local, and always restored by callers.
use crate::game::{BoardObjective, Game};
use domain::{manager::Manager, message::InboxMessage};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControlledClubs {
    #[serde(default)]
    pub clubs: BTreeSet<String>,
    #[serde(default)]
    pub inactive: BTreeMap<String, ManagerState>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagerState {
    pub manager: Manager,
    #[serde(default)]
    pub messages: Vec<InboxMessage>,
    #[serde(default)]
    pub objectives: Vec<BoardObjective>,
    #[serde(default)]
    pub emitted: BTreeSet<String>,
    #[serde(default)]
    pub days_since_job_offer: Option<u32>,
}
impl Game {
    pub fn is_controlled(&self, club: &str) -> bool {
        self.manager.team_id.as_deref() == Some(club) || self.controlled.clubs.contains(club)
    }
    pub fn controlled_clubs(&self) -> Vec<String> {
        let mut ids = self.controlled.clubs.clone();
        ids.extend(self.manager.team_id.clone());
        ids.into_iter().collect()
    }
    pub fn swap_controlled(&mut self, club: &str) {
        if self.manager.team_id.as_deref() == Some(club) {
            return;
        }
        let mut state = self
            .controlled
            .inactive
            .remove(club)
            .expect("saved human context");
        let previous = self.manager.team_id.clone().expect("human club");
        std::mem::swap(&mut self.manager, &mut state.manager);
        std::mem::swap(&mut self.messages, &mut state.messages);
        std::mem::swap(&mut self.board_objectives, &mut state.objectives);
        std::mem::swap(&mut self.emitted_events, &mut state.emitted);
        std::mem::swap(
            &mut self.days_since_last_job_offer,
            &mut state.days_since_job_offer,
        );
        self.manager_id = self.manager.id.clone();
        self.controlled.inactive.insert(previous, state);
    }
    pub fn for_each_controlled(&mut self, mut work: impl FnMut(&mut Self)) {
        let original = self.manager.team_id.clone();
        for club in self.controlled_clubs() {
            self.swap_controlled(&club);
            work(self);
            self.sync_user_manager_record();
        }
        if let Some(club) = original {
            self.swap_controlled(&club);
        }
    }
    pub fn deliver_to_club(&mut self, club: &str, message: InboxMessage) {
        if self.manager.team_id.as_deref() == Some(club) {
            self.messages.push(message);
        } else if let Some(state) = self.controlled.inactive.get_mut(club) {
            state.messages.push(message);
        }
    }
}
