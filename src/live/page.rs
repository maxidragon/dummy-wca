//! Not a WCA page: a plain view of a generated competition's live results,
//! for watching what a client enters. It reads the same state as the live
//! results API.

use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;

use super::model::{Definition, LiveResult, Round};
use super::{definition, rounds};
use crate::state::SharedState;

struct RoundRow {
    id: String,
    state: &'static str,
    competitors: usize,
    completed: usize,
    selected: bool,
}

struct ResultRow {
    pos: String,
    name: String,
    registration_id: u64,
    registrant_id: String,
    attempts: Vec<String>,
    average: String,
    best: String,
    /// `advancing`, `questionable`, or empty.
    class: &'static str,
    locked: bool,
    quit: bool,
}

#[derive(Template)]
#[template(path = "live.html")]
struct LiveTemplate {
    competition_id: String,
    competition_name: String,
    scoretaking_software: &'static str,
    rounds: Vec<RoundRow>,
    round_id: String,
    round_format: String,
    round_state: &'static str,
    results: Vec<ResultRow>,
    refresh: bool,
}

#[derive(Deserialize)]
pub struct PageOptions {
    round: Option<String>,
    refresh: Option<String>,
}

/// A WCA result value as people read it: a time, a move count, a
/// multi-blind score, DNF or DNS; nothing for no attempt.
pub fn format_value(event_id: &str, value: i64, is_average: bool) -> String {
    match value {
        0 => String::new(),
        -1 => "DNF".to_string(),
        -2 => "DNS".to_string(),
        _ if event_id == "333fm" => {
            if is_average {
                format!("{:.2}", value as f64 / 100.0)
            } else {
                value.to_string()
            }
        }
        _ if event_id == "333mbf" || event_id == "333mbo" => {
            let missed = value % 100;
            let seconds = (value / 100) % 100_000;
            let points = 99 - (value / 10_000_000) % 100;
            let solved = points + missed;
            format!(
                "{}/{} {}:{:02}",
                solved,
                solved + missed,
                seconds / 60,
                seconds % 60
            )
        }
        _ => {
            let (minutes, centiseconds) = (value / 6000, value % 6000);
            if minutes > 0 {
                format!(
                    "{}:{:02}.{:02}",
                    minutes,
                    centiseconds / 100,
                    centiseconds % 100
                )
            } else {
                format!("{}.{:02}", centiseconds / 100, centiseconds % 100)
            }
        }
    }
}

fn result_row(definition: &Definition, round: &Round, result: &LiveResult) -> ResultRow {
    let registration = definition.registration(result.registration_id);
    let mut attempts: Vec<String> = result
        .attempts
        .iter()
        .map(|attempt| format_value(&round.event_id, attempt.value, false))
        .collect();
    attempts.resize(round.format.expected_solve_count, String::new());
    ResultRow {
        pos: result
            .local_pos
            .map(|pos| pos.to_string())
            .unwrap_or_default(),
        name: registration.map_or_else(
            || format!("Registration {}", result.registration_id),
            |registration| registration.name.clone(),
        ),
        registration_id: result.registration_id,
        registrant_id: registration
            .and_then(|registration| registration.registrant_id)
            .map(|id| id.to_string())
            .unwrap_or_default(),
        attempts,
        average: format_value(&round.event_id, result.average, true),
        best: format_value(&round.event_id, result.best, false),
        class: if result.advancing {
            "advancing"
        } else if result.advancing_questionable {
            "questionable"
        } else {
            ""
        },
        locked: result.locked,
        quit: result.quit,
    }
}

pub async fn live_page(
    State(state): State<SharedState>,
    Path(competition_id): Path<String>,
    Query(options): Query<PageOptions>,
) -> Response {
    let Some(competition) = state.generated(&competition_id) else {
        return (
            StatusCode::NOT_FOUND,
            "Live results are kept for generated competitions only",
        )
            .into_response();
    };
    let definition = definition(&state, &competition);
    let template = state.live.with(&competition.id, |results| {
        let rounds = rounds(&definition, results);
        let states: Vec<_> = (0..definition.rounds.len())
            .map(|index| rounds.state(index))
            .collect();
        // The asked round, else the first open one, else the first.
        let selected = options
            .round
            .as_deref()
            .and_then(|id| definition.round(id))
            .or_else(|| states.iter().position(|state| state.name() == "open"))
            .unwrap_or(0);
        let round_rows = definition
            .rounds
            .iter()
            .enumerate()
            .map(|(index, round)| RoundRow {
                id: round.wcif_id.clone(),
                state: states[index].name(),
                competitors: rounds.results[index].len(),
                completed: round.completed_competitors(rounds.results[index]),
                selected: index == selected,
            })
            .collect();
        let round = definition.rounds.get(selected);
        let mut result_rows: Vec<ResultRow> = round
            .map(|round| {
                rounds.results[selected]
                    .iter()
                    .map(|result| result_row(&definition, round, result))
                    .collect()
            })
            .unwrap_or_default();
        result_rows.sort_by(|a, b| {
            let pos = |row: &ResultRow| row.pos.parse::<u32>().unwrap_or(u32::MAX);
            pos(a).cmp(&pos(b)).then_with(|| a.name.cmp(&b.name))
        });
        LiveTemplate {
            competition_id: competition.id.clone(),
            competition_name: competition.name.clone(),
            scoretaking_software: competition.scoretaking_software,
            rounds: round_rows,
            round_id: round.map(|round| round.wcif_id.clone()).unwrap_or_default(),
            round_format: round
                .map(|round| round.format.id.to_string())
                .unwrap_or_default(),
            round_state: states.get(selected).map_or("", |state| state.name()),
            results: result_rows,
            refresh: options.refresh.as_deref() != Some("0"),
        }
    });
    match template.render() {
        Ok(html) => Html(html).into_response(),
        Err(_) => StatusCode::INTERNAL_SERVER_ERROR.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::format_value;

    #[test]
    fn values_read_like_on_the_wca() {
        assert_eq!(format_value("333", 0, false), "");
        assert_eq!(format_value("333", -1, false), "DNF");
        assert_eq!(format_value("333", -2, false), "DNS");
        assert_eq!(format_value("333", 987, false), "9.87");
        assert_eq!(format_value("333", 6543, false), "1:05.43");
        assert_eq!(format_value("333fm", 27, false), "27");
        assert_eq!(format_value("333fm", 2567, true), "25.67");
        // 9 solved of 10 in 58:20: points 8, so DD = 91.
        assert_eq!(format_value("333mbf", 910_350_001, false), "9/10 58:20");
    }
}
