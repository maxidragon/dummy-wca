use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use chrono::{DateTime, Utc};
use serde_json::{Map, Value};

use crate::generator::{self, Competition, GeneratorConfig};

pub type SharedState = Arc<AppState>;

type PersonPatch = Map<String, Value>;

/// Server settings that aren't about the generated competition.
#[derive(Clone, Debug)]
pub struct Settings {
    /// How long a token from `POST /oauth/token` lasts. The WCA's last 2 hours.
    pub token_lifetime_seconds: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings::from_vars(|_| None).expect("defaults are valid")
    }
}

impl Settings {
    pub fn from_env() -> Result<Self, String> {
        Settings::from_vars(|key| std::env::var(key).ok())
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let number = |key: &str, default: u32, min: u32| -> Result<u32, String> {
            match get(key).map(|value| value.trim().to_string()) {
                Some(value) if !value.is_empty() => match value.parse::<u32>() {
                    Ok(number) if number >= min => Ok(number),
                    _ => Err(format!(
                        "{} must be a whole number of at least {}, got {:?}",
                        key, min, value
                    )),
                },
                _ => Ok(default),
            }
        };
        Ok(Settings {
            token_lifetime_seconds: number("TOKEN_LIFETIME_SECONDS", 7200, 1)?,
        })
    }
}

pub struct AppState {
    pub config: GeneratorConfig,
    pub settings: Settings,
    /// When the server started; "today" is taken from this once, so the
    /// competition doesn't move at midnight under a database seeded from it.
    pub started_at: DateTime<Utc>,
    competition: Arc<Competition>,
    /// Competitions generated from the page at `/` while the server runs,
    /// oldest first. Kept in memory only, like patches.
    created: RwLock<Vec<Arc<Competition>>>,
    /// Persons saved with `PATCH .../wcif`, by competition id and registrant id.
    /// Kept in memory only: a restart or `POST /dummy/reset` clears them.
    patches: RwLock<HashMap<String, HashMap<i64, PersonPatch>>>,
    /// `schedule`, `events` and `extensions` saved the same way, by competition.
    document_patches: RwLock<HashMap<String, Map<String, Value>>>,
    /// Refresh tokens already exchanged: like on the WCA, each works once.
    used_refresh_tokens: Mutex<HashSet<String>>,
    /// Makes every refresh token distinct, across restarts too.
    next_nonce: AtomicU64,
}

impl AppState {
    pub fn new(config: GeneratorConfig) -> Self {
        AppState::started_at(config, Utc::now())
    }

    /// Generates the competition for the start date as of `now`: the pinned
    /// `GENERATED_START_DATE`, or today in the venue timezone.
    pub fn started_at(config: GeneratorConfig, now: DateTime<Utc>) -> Self {
        let competition = Arc::new(generator::generate(&config, config.start_date_at(now)));
        AppState {
            config,
            started_at: now,
            competition,
            created: RwLock::new(Vec::new()),
            patches: RwLock::new(HashMap::new()),
            document_patches: RwLock::new(HashMap::new()),
            used_refresh_tokens: Mutex::new(HashSet::new()),
            next_nonce: AtomicU64::new(now.timestamp_micros().unsigned_abs()),
            settings: Settings::default(),
        }
    }

    pub fn with_settings(mut self, settings: Settings) -> Self {
        self.settings = settings;
        self
    }

    pub fn nonce(&self) -> u64 {
        self.next_nonce.fetch_add(1, Ordering::Relaxed)
    }

    /// Marks a refresh token as used. Returns false if it already was.
    pub fn use_refresh_token(&self, token: &str) -> bool {
        self.used_refresh_tokens
            .lock()
            .expect("refresh token lock is never poisoned")
            .insert(token.to_string())
    }

    /// The generated competition. It stays the same until the server restarts.
    pub fn competition(&self) -> Arc<Competition> {
        self.competition.clone()
    }

    /// Every generated competition: the one from the `GENERATED_*` settings
    /// first, then those created from the page at `/`.
    pub fn competitions(&self) -> Vec<Arc<Competition>> {
        let created = self
            .created
            .read()
            .expect("competition lock is never poisoned");
        std::iter::once(self.competition())
            .chain(created.iter().cloned())
            .collect()
    }

    /// The generated competition with this id.
    pub fn generated(&self, id: &str) -> Option<Arc<Competition>> {
        self.competitions()
            .into_iter()
            .find(|competition| competition.id == id)
    }

    /// Adds a competition generated at runtime. Fails if its id is taken.
    pub fn create(&self, competition: Competition) -> Result<Arc<Competition>, String> {
        let mut created = self
            .created
            .write()
            .expect("competition lock is never poisoned");
        let taken = competition.id == self.competition.id
            || created.iter().any(|other| other.id == competition.id)
            || std::path::Path::new("data/competitions")
                .join(&competition.id)
                .exists();
        if taken {
            return Err(format!(
                "A competition with id {} already exists; pick another name or start date",
                competition.id
            ));
        }
        let competition = Arc::new(competition);
        created.push(competition.clone());
        Ok(competition)
    }

    /// Removes a competition created at runtime, with its patches. The one
    /// from the settings can't be removed. Returns whether it existed.
    pub fn remove(&self, id: &str) -> bool {
        let mut created = self
            .created
            .write()
            .expect("competition lock is never poisoned");
        let before = created.len();
        created.retain(|competition| competition.id != id);
        let removed = created.len() != before;
        if removed {
            self.reset(Some(id));
        }
        removed
    }

    pub fn save_patch(
        &self,
        competition_id: &str,
        persons: Vec<PersonPatch>,
        document: Map<String, Value>,
    ) {
        if !document.is_empty() {
            self.document_patches
                .write()
                .expect("patch lock is never poisoned")
                .entry(competition_id.to_string())
                .or_default()
                .extend(document);
        }
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
        let documents = self
            .document_patches
            .read()
            .expect("patch lock is never poisoned");
        if let (Some(saved), Some(document)) = (documents.get(competition_id), wcif.as_object_mut())
        {
            document.extend(saved.clone());
        }
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
        let mut documents = self
            .document_patches
            .write()
            .expect("patch lock is never poisoned");
        match competition_id {
            Some(id) => {
                let had_documents = documents.remove(id).is_some();
                usize::from(patches.remove(id).is_some() || had_documents)
            }
            None => {
                let ids: std::collections::HashSet<_> =
                    patches.keys().chain(documents.keys()).cloned().collect();
                patches.clear();
                documents.clear();
                ids.len()
            }
        }
    }
}
