use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::generator::{self, Competition, GeneratorConfig};

pub type SharedState = Arc<AppState>;

type PersonPatch = serde_json::Map<String, Value>;

pub struct AppState {
    pub config: GeneratorConfig,
    now: fn() -> DateTime<Utc>,
    /// Today's competition; rebuilt when the start date changes.
    cache: Mutex<Option<Arc<Competition>>>,
    /// Persons saved with `PATCH .../wcif`, by competition id and registrant id.
    /// Kept in memory only: a restart or `POST /dummy/reset` clears them.
    patches: RwLock<HashMap<String, HashMap<i64, PersonPatch>>>,
}

impl AppState {
    pub fn new(config: GeneratorConfig) -> Self {
        AppState::with_clock(config, Utc::now)
    }

    pub fn with_clock(config: GeneratorConfig, now: fn() -> DateTime<Utc>) -> Self {
        AppState {
            config,
            now,
            cache: Mutex::new(None),
            patches: RwLock::new(HashMap::new()),
        }
    }

    /// The generated competition as of now.
    pub fn competition(&self) -> Arc<Competition> {
        let start_date = self.config.start_date_at((self.now)());
        let mut cache = self.cache.lock().expect("cache lock is never poisoned");
        match cache.as_ref() {
            Some(competition) if competition.start_date == start_date => competition.clone(),
            _ => {
                let competition = Arc::new(generator::generate(&self.config, start_date));
                *cache = Some(competition.clone());
                competition
            }
        }
    }

    /// The generated competition, if `id` is its current id.
    pub fn generated(&self, id: &str) -> Option<Arc<Competition>> {
        let competition = self.competition();
        (competition.id == id).then_some(competition)
    }

    pub fn save_patch(&self, competition_id: &str, persons: Vec<PersonPatch>) {
        let mut patches = self.patches.write().expect("patch lock is never poisoned");
        let saved = patches.entry(competition_id.to_string()).or_default();
        for person in persons {
            let registrant_id = person["registrantId"]
                .as_i64()
                .expect("validated before saving");
            saved.entry(registrant_id).or_default().extend(person);
        }
    }

    /// Applies saved patches to a WCIF of that competition.
    pub fn apply_patches(&self, competition_id: &str, wcif: &mut Value) {
        let patches = self.patches.read().expect("patch lock is never poisoned");
        let (Some(saved), Some(persons)) =
            (patches.get(competition_id), wcif["persons"].as_array_mut())
        else {
            return;
        };
        for person in persons.iter_mut() {
            let Some(patch) = person["registrantId"]
                .as_i64()
                .and_then(|id| saved.get(&id))
            else {
                continue;
            };
            if let Some(person) = person.as_object_mut() {
                for (key, value) in patch {
                    person.insert(key.clone(), value.clone());
                }
            }
        }
    }

    pub fn patched_persons(&self, competition_id: &str) -> usize {
        let patches = self.patches.read().expect("patch lock is never poisoned");
        patches.get(competition_id).map_or(0, HashMap::len)
    }

    /// Drops saved patches for one competition, or for all. Returns how many
    /// competitions had patches.
    pub fn reset(&self, competition_id: Option<&str>) -> usize {
        let mut patches = self.patches.write().expect("patch lock is never poisoned");
        match competition_id {
            Some(id) => usize::from(patches.remove(id).is_some()),
            None => {
                let count = patches.len();
                patches.clear();
                count
            }
        }
    }
}
