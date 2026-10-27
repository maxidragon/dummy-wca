//! The page at `/`: lists every competition the server knows and generates
//! new ones while it runs.

use std::collections::HashMap;

use askama::Template;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use chrono::Utc;

use crate::generator::{self, events::EVENTS, names::HOSTS, GeneratorConfig};
use crate::state::SharedState;
use crate::utils::read_json_file;

/// Default names for new competitions, so the form can be sent as it is.
const NAMES: [&str; 8] = [
    "Alpha Test Open",
    "Bravo Test Open",
    "Charlie Test Open",
    "Delta Test Open",
    "Echo Test Open",
    "Foxtrot Test Open",
    "Golf Test Open",
    "Hotel Test Open",
];

struct GeneratedRow {
    id: String,
    name: String,
    dates: String,
    timezone: String,
    competitors: usize,
    accepted: usize,
    rooms: usize,
    events: String,
    removable: bool,
}

struct SnapshotRow {
    id: String,
    name: String,
    dates: String,
}

struct EventRow {
    id: &'static str,
    name: &'static str,
    rounds: u32,
    default_rounds: u32,
}

struct TimezoneOption {
    name: &'static str,
    selected: bool,
}

/// What the form shows: the defaults, or what was sent when it failed.
struct FormValues {
    name: String,
    start_date: String,
    days: String,
    competitors: String,
    rooms: String,
    seed: String,
    timezones: Vec<TimezoneOption>,
    events: Vec<EventRow>,
}

#[derive(Template)]
#[template(path = "index.html")]
struct IndexTemplate {
    error: String,
    generated: Vec<GeneratedRow>,
    snapshots: Vec<SnapshotRow>,
    form: FormValues,
}

fn render(template: IndexTemplate, status: StatusCode) -> Response {
    match template.render() {
        Ok(html) => (status, Html(html)).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

fn generated_rows(state: &SharedState) -> Vec<GeneratedRow> {
    state
        .competitions()
        .iter()
        .enumerate()
        .map(|(index, competition)| GeneratedRow {
            id: competition.id.clone(),
            name: competition.name.clone(),
            dates: dates(
                &competition.start_date.format("%Y-%m-%d").to_string(),
                &competition.end_date().format("%Y-%m-%d").to_string(),
            ),
            timezone: competition.timezone.name().to_string(),
            competitors: competition.persons.len(),
            accepted: competition.accepted().count(),
            rooms: competition.venue.rooms.len(),
            events: competition
                .events
                .iter()
                .map(|event| format!("{} ×{}", event.info.id, event.rounds.len()))
                .collect::<Vec<_>>()
                .join(", "),
            // The first one comes from the GENERATED_* settings; tests rely on it.
            removable: index > 0,
        })
        .collect()
}

fn snapshot_rows() -> Vec<SnapshotRow> {
    let mut ids: Vec<String> = std::fs::read_dir("data/competitions")
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    ids.sort();
    ids.into_iter()
        .map(|id| {
            let info = read_json_file(&format!("data/competitions/{}/competition.json", id))
                .unwrap_or_default();
            SnapshotRow {
                name: info["name"].as_str().unwrap_or(&id).to_string(),
                dates: dates(
                    info["start_date"].as_str().unwrap_or_default(),
                    info["end_date"].as_str().unwrap_or_default(),
                ),
                id,
            }
        })
        .collect()
}

fn dates(start: &str, end: &str) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{} – {}", start, end)
    }
}

fn default_form(state: &SharedState) -> FormValues {
    let taken: Vec<String> = state
        .competitions()
        .iter()
        .map(|competition| competition.name.clone())
        .collect();
    let name = NAMES
        .iter()
        .find(|name| !taken.iter().any(|taken| taken.starts_with(*name)))
        .map_or_else(
            || format!("Test Open {}", taken.len()),
            |name| name.to_string(),
        );
    let defaults = GeneratorConfig::default();
    let default_rounds = |id: &str| {
        defaults
            .events
            .iter()
            .find(|(event, _)| event.id == id)
            .map_or(0, |(_, rounds)| *rounds)
    };
    FormValues {
        name,
        start_date: defaults
            .start_date_at(Utc::now())
            .format("%Y-%m-%d")
            .to_string(),
        days: defaults.days.to_string(),
        competitors: defaults.competitors.to_string(),
        rooms: defaults.rooms.to_string(),
        seed: (state.competitions().len() as u64 + 1).to_string(),
        timezones: timezones(defaults.timezone.name()),
        events: EVENTS
            .iter()
            .map(|event| EventRow {
                id: event.id,
                name: event.name,
                rounds: default_rounds(event.id),
                default_rounds: default_rounds(event.id),
            })
            .collect(),
    }
}

fn timezones(selected: &str) -> Vec<TimezoneOption> {
    HOSTS
        .iter()
        .map(|host| TimezoneOption {
            name: host.timezone,
            selected: host.timezone == selected,
        })
        .collect()
}

/// The form again, filled in with what was sent.
fn sent_form(state: &SharedState, fields: &HashMap<String, String>) -> FormValues {
    let mut form = default_form(state);
    let field = |key: &str, default: &str| {
        fields
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.to_string())
    };
    form.name = field("name", &form.name);
    form.start_date = field("start_date", &form.start_date);
    form.days = field("days", &form.days);
    form.competitors = field("competitors", &form.competitors);
    form.rooms = field("rooms", &form.rooms);
    form.seed = field("seed", &form.seed);
    if let Some(timezone) = fields.get("timezone") {
        form.timezones = timezones(timezone);
    }
    for event in &mut form.events {
        event.rounds = rounds_field(fields, event.id);
    }
    form
}

fn rounds_field(fields: &HashMap<String, String>, event_id: &str) -> u32 {
    fields
        .get(&format!("event_{}", event_id))
        .and_then(|rounds| rounds.parse().ok())
        .unwrap_or(0)
}

/// Turns the form into the same settings the `GENERATED_*` variables give,
/// so both are checked the same way.
fn config_from_form(fields: &HashMap<String, String>) -> Result<GeneratorConfig, String> {
    let events: Vec<String> = EVENTS
        .iter()
        .map(|event| (event.id, rounds_field(fields, event.id)))
        .filter(|(_, rounds)| *rounds > 0)
        .map(|(id, rounds)| format!("{}:{}", id, rounds))
        .collect();
    if events.is_empty() {
        return Err("Pick at least one event".to_string());
    }
    let field = |key: &str| fields.get(key).cloned();
    GeneratorConfig::from_vars(|key| match key {
        "GENERATED_NAME" => field("name"),
        "GENERATED_START_DATE" => field("start_date"),
        "GENERATED_DAYS" => field("days"),
        "GENERATED_COMPETITORS" => field("competitors"),
        "GENERATED_ROOMS" => field("rooms"),
        "GENERATED_TIMEZONE" => field("timezone"),
        "GENERATED_SEED" => field("seed"),
        "GENERATED_EVENTS" => Some(events.join(",")),
        _ => None,
    })
}

/// The checks name the variables; the form names the fields.
fn form_wording(message: &str) -> String {
    [
        ("GENERATED_NAME", "Name"),
        ("GENERATED_START_DATE", "Start date"),
        ("GENERATED_DAYS", "Days"),
        ("GENERATED_COMPETITORS", "Competitors"),
        ("GENERATED_ROOMS", "Rooms"),
        ("GENERATED_TIMEZONE", "Timezone"),
        ("GENERATED_SEED", "Seed"),
        ("GENERATED_EVENTS", "Events"),
    ]
    .iter()
    .fold(message.to_string(), |message, (variable, field)| {
        message.replace(variable, field)
    })
}

pub async fn index_handler(State(state): State<SharedState>) -> Response {
    render(
        IndexTemplate {
            error: String::new(),
            generated: generated_rows(&state),
            snapshots: snapshot_rows(),
            form: default_form(&state),
        },
        StatusCode::OK,
    )
}

pub async fn create_handler(
    State(state): State<SharedState>,
    Form(fields): Form<HashMap<String, String>>,
) -> Response {
    let created = config_from_form(&fields).and_then(|config| {
        let start_date = config.start_date_at(Utc::now());
        generator::check(&config, start_date)?;
        state.create(generator::generate(&config, start_date))
    });
    match created {
        Ok(_) => Redirect::to("/").into_response(),
        Err(error) => render(
            IndexTemplate {
                error: form_wording(&error),
                generated: generated_rows(&state),
                snapshots: snapshot_rows(),
                form: sent_form(&state, &fields),
            },
            StatusCode::BAD_REQUEST,
        ),
    }
}

pub async fn remove_handler(State(state): State<SharedState>, Path(id): Path<String>) -> Response {
    if state.remove(&id) {
        Redirect::to("/").into_response()
    } else {
        (
            StatusCode::NOT_FOUND,
            "Only competitions created from this page can be removed",
        )
            .into_response()
    }
}
