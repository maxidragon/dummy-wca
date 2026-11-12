use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

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
    /// Requests per minute per client to each `/api/v1` controller, like the
    /// WCA's 60. 0 turns the limit off.
    pub api_rate_limit_per_minute: u32,
    /// How long after its 202 a live results write is applied.
    pub live_result_delay_ms: u64,
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
            api_rate_limit_per_minute: number("API_RATE_LIMIT_PER_MINUTE", 60, 0)?,
            live_result_delay_ms: number("LIVE_RESULT_DELAY_MS", 500, 0)?.into(),
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
    /// Events set with `PATCH /api/v1/registrations/:id`, by competition and
    /// registration id. In memory only, like patches.
    registration_events: RwLock<HashMap<String, HashMap<u64, Vec<String>>>>,
    /// User ids of each competition's scoretakers, in the order they were added.
    scoretakers: RwLock<HashMap<String, Vec<u64>>>,
    /// Requests counted for the rate limit: when the window started, and how many.
    request_counts: Mutex<HashMap<String, (Instant, u32)>>,
    /// Competitions created so far, so each gets its own registration ids.
    created_count: AtomicU64,
    /// Live results entered through the live results API.
    pub live: crate::live::Store,
}

/// Registration ids of competitions created at runtime are moved this far
/// apart, so an id names one registration, as on the WCA.
const REGISTRATION_ID_SPACING: u64 = 10_000;

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
            registration_events: RwLock::new(HashMap::new()),
            scoretakers: RwLock::new(HashMap::new()),
            request_counts: Mutex::new(HashMap::new()),
            created_count: AtomicU64::new(0),
            live: crate::live::Store::default(),
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
        let mut competition = competition;
        let offset =
            (self.created_count.fetch_add(1, Ordering::Relaxed) + 1) * REGISTRATION_ID_SPACING;
        for person in &mut competition.persons {
            person.registration.wca_registration_id += offset;
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

    /// Counts a request against `limit` per `window`. Returns false once the
    /// limit is used up, until the window is over.
    pub fn count_request(&self, key: &str, limit: u32, window: Duration, now: Instant) -> bool {
        let mut counts = self
            .request_counts
            .lock()
            .expect("rate limit lock is never poisoned");
        let (started, count) = counts.entry(key.to_string()).or_insert((now, 0));
        if now.duration_since(*started) >= window {
            *started = now;
            *count = 0;
        }
        *count += 1;
        *count <= limit
    }

    /// Replaces a registration's events.
    pub fn set_registration_events(
        &self,
        competition_id: &str,
        registration_id: u64,
        event_ids: Vec<String>,
    ) {
        self.registration_events
            .write()
            .expect("registration lock is never poisoned")
            .entry(competition_id.to_string())
            .or_default()
            .insert(registration_id, event_ids);
    }

    /// A registration's events, if they were changed.
    pub fn registration_events(
        &self,
        competition_id: &str,
        registration_id: u64,
    ) -> Option<Vec<String>> {
        self.registration_events
            .read()
            .expect("registration lock is never poisoned")
            .get(competition_id)
            .and_then(|events| events.get(&registration_id).cloned())
    }

    pub fn scoretakers(&self, competition_id: &str) -> Vec<u64> {
        self.scoretakers
            .read()
            .expect("scoretaker lock is never poisoned")
            .get(competition_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Adds a scoretaker unless they already are one.
    pub fn add_scoretaker(&self, competition_id: &str, user_id: u64) {
        let mut scoretakers = self
            .scoretakers
            .write()
            .expect("scoretaker lock is never poisoned");
        let list = scoretakers.entry(competition_id.to_string()).or_default();
        if !list.contains(&user_id) {
            list.push(user_id);
        }
    }

    /// Returns whether they were a scoretaker.
    pub fn remove_scoretaker(&self, competition_id: &str, user_id: u64) -> bool {
        let mut scoretakers = self
            .scoretakers
            .write()
            .expect("scoretaker lock is never poisoned");
        let Some(list) = scoretakers.get_mut(competition_id) else {
            return false;
        };
        let before = list.len();
        list.retain(|id| *id != user_id);
        list.len() != before
    }

    /// Applies saved patches to a WCIF of that competition.
    pub fn apply_patches(&self, competition_id: &str, wcif: &mut Value) {
        self.apply_registration_events(competition_id, wcif);
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

    fn apply_registration_events(&self, competition_id: &str, wcif: &mut Value) {
        let changed = self
            .registration_events
            .read()
            .expect("registration lock is never poisoned");
        let (Some(changed), Some(persons)) =
            (changed.get(competition_id), wcif["persons"].as_array_mut())
        else {
            return;
        };
        for person in persons.iter_mut() {
            let registration = &mut person["registration"];
            if let Some(event_ids) = registration["wcaRegistrationId"]
                .as_u64()
                .and_then(|id| changed.get(&id))
            {
                registration["eventIds"] = Value::from(event_ids.clone());
            }
        }
    }

    pub fn patched_persons(&self, competition_id: &str) -> usize {
        let patches = self.patches.read().expect("patch lock is never poisoned");
        patches.get(competition_id).map_or(0, HashMap::len)
    }

    /// Drops saved patches, registration changes, scoretakers and live
    /// results for one competition, or for all. Returns how many competitions
    /// had any.
    pub fn reset(&self, competition_id: Option<&str>) -> usize {
        let live = self.live.reset(competition_id);
        let mut patches = self.patches.write().expect("patch lock is never poisoned");
        let mut documents = self
            .document_patches
            .write()
            .expect("patch lock is never poisoned");
        let mut events = self
            .registration_events
            .write()
            .expect("registration lock is never poisoned");
        let mut scoretakers = self
            .scoretakers
            .write()
            .expect("scoretaker lock is never poisoned");
        match competition_id {
            Some(id) => {
                let had_documents = documents.remove(id).is_some();
                let had_events = events.remove(id).is_some();
                let had_scoretakers = scoretakers.remove(id).is_some();
                usize::from(
                    patches.remove(id).is_some()
                        || had_documents
                        || had_events
                        || had_scoretakers
                        || !live.is_empty(),
                )
            }
            None => {
                let ids: HashSet<_> = patches
                    .keys()
                    .chain(documents.keys())
                    .chain(events.keys())
                    .chain(scoretakers.keys())
                    .chain(live.iter())
                    .cloned()
                    .collect();
                patches.clear();
                documents.clear();
                events.clear();
                scoretakers.clear();
                ids.len()
            }
        }
    }
}
