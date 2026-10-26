//! A generated, invented test competition. The same configuration always
//! gives the same data; only the dates follow the start date.

mod assignments;
pub mod config;
pub mod events;
pub mod model;
pub mod names;
mod persons;
mod rng;
mod rounds;
mod schedule;
pub mod wcif;

use chrono::{Datelike, Duration, NaiveDate, NaiveTime, Timelike};

pub use config::GeneratorConfig;
pub use model::Competition;

/// `Dummy Cubing Open` in 2026 is `DummyCubingOpen2026`.
pub fn competition_id(name: &str, year: i32) -> String {
    let base: String = name.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    format!("{}{}", base, year)
}

pub fn generate(config: &GeneratorConfig, start_date: NaiveDate) -> Competition {
    let year = start_date.year();
    let mut persons = persons::people(config);
    persons::register(config, &mut persons);
    persons::personal_bests(config, &mut persons);

    let accepted = persons.iter().filter(|person| person.is_accepted()).count() as u32;
    let stations = rounds::stations(accepted, config.rooms);
    let events = rounds::build(config, &persons, stations);
    let venue = schedule::build(config, start_date, &events, stations);
    assignments::assign(config, &mut persons, &events, &venue);

    let local = |days_before: i64, hour: u32, minute: u32| {
        (start_date - Duration::days(days_before))
            .and_time(NaiveTime::from_hms_opt(hour, minute, 0).expect("valid time"))
    };

    Competition {
        id: competition_id(&config.name, year),
        name: format!("{} {}", config.name, year),
        start_date,
        number_of_days: config.days,
        timezone: config.timezone,
        host: config.host,
        persons,
        events,
        venue,
        registration_open: local(28, 19, 0),
        registration_close: local(7, 23, 59),
        competitor_limit: accepted.div_ceil(10) * 10,
    }
}

/// Fails when the configured rounds don't fit in the configured days, so a
/// bad configuration stops the server at startup instead of serving nonsense.
pub fn check(config: &GeneratorConfig, start_date: NaiveDate) -> Result<(), String> {
    let competition = generate(config, start_date);
    let last_day = competition.end_date();
    for activity in competition.activities() {
        let ends_too_late = activity.end.date() != activity.start.date()
            || (activity.end.hour() * 60 + activity.end.minute()) as i64 > schedule::HARD_END
            || activity.start.date() > last_day;
        if ends_too_late {
            return Err(format!(
                "the schedule doesn't fit: {} ends at {}; raise GENERATED_DAYS or GENERATED_ROOMS, or lower GENERATED_COMPETITORS or the rounds in GENERATED_EVENTS",
                activity.code, activity.end
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
