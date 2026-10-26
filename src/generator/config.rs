use chrono::{DateTime, NaiveDate, Utc};
use chrono_tz::Tz;

use super::events::{self, EventInfo};
use super::names::{self, Host};

/// What to generate. Read once at startup from `GENERATED_*` env vars.
#[derive(Clone)]
pub struct GeneratorConfig {
    pub name: String,
    /// Pinned start date; `None` means today in the venue timezone when the
    /// server starts.
    pub start_date: Option<NaiveDate>,
    pub days: u32,
    pub competitors: u32,
    pub events: Vec<(&'static EventInfo, u32)>,
    pub rooms: u32,
    pub timezone: Tz,
    pub host: &'static Host,
    pub seed: u64,
}

const DEFAULT_EVENTS: &str =
    "333:3,222:2,444:2,pyram:2,skewb:1,333oh:2,clock:1,333bf:1,sq1:1,minx:1";

impl Default for GeneratorConfig {
    fn default() -> Self {
        GeneratorConfig::from_vars(|_| None).expect("defaults are valid")
    }
}

impl GeneratorConfig {
    pub fn from_env() -> Result<Self, String> {
        GeneratorConfig::from_vars(|key| std::env::var(key).ok())
    }

    pub fn from_vars(get: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let read = |key: &str, default: &str| {
            get(key)
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
                .unwrap_or_else(|| default.to_string())
        };

        let name = read("GENERATED_NAME", "Dummy Cubing Open");
        if !name.chars().any(|c| c.is_ascii_alphabetic()) {
            return Err(format!(
                "GENERATED_NAME must contain letters, got {:?}",
                name
            ));
        }

        let start_date = match get("GENERATED_START_DATE").filter(|value| !value.trim().is_empty())
        {
            Some(value) => Some(NaiveDate::parse_from_str(value.trim(), "%Y-%m-%d").map_err(
                |_| format!("GENERATED_START_DATE must be YYYY-MM-DD, got {:?}", value),
            )?),
            None => None,
        };

        let timezone_name = read("GENERATED_TIMEZONE", "Europe/Warsaw");
        let timezone: Tz = timezone_name.parse().map_err(|_| {
            format!(
                "GENERATED_TIMEZONE is not an IANA timezone: {:?}",
                timezone_name
            )
        })?;
        let host = names::host(timezone.name()).ok_or_else(|| {
            let supported: Vec<&str> = names::HOSTS.iter().map(|host| host.timezone).collect();
            format!(
                "GENERATED_TIMEZONE {:?} has no host country; use one of: {}",
                timezone_name,
                supported.join(", ")
            )
        })?;

        Ok(GeneratorConfig {
            name,
            start_date,
            days: parse_number(&read("GENERATED_DAYS", "2"), "GENERATED_DAYS", 1, 4)?,
            competitors: parse_number(
                &read("GENERATED_COMPETITORS", "60"),
                "GENERATED_COMPETITORS",
                2,
                1000,
            )?,
            events: parse_events(&read("GENERATED_EVENTS", DEFAULT_EVENTS))?,
            rooms: parse_number(&read("GENERATED_ROOMS", "2"), "GENERATED_ROOMS", 1, 4)?,
            timezone,
            host,
            seed: read("GENERATED_SEED", "1")
                .parse()
                .map_err(|_| "GENERATED_SEED must be a non-negative integer".to_string())?,
        })
    }

    /// The start date for a server started at `now`: the pinned date, or
    /// today in the venue timezone.
    pub fn start_date_at(&self, now: DateTime<Utc>) -> NaiveDate {
        self.start_date
            .unwrap_or_else(|| now.with_timezone(&self.timezone).date_naive())
    }
}

fn parse_number(value: &str, key: &str, min: u32, max: u32) -> Result<u32, String> {
    match value.parse::<u32>() {
        Ok(number) if (min..=max).contains(&number) => Ok(number),
        _ => Err(format!(
            "{} must be a number from {} to {}, got {:?}",
            key, min, max, value
        )),
    }
}

fn parse_events(value: &str) -> Result<Vec<(&'static EventInfo, u32)>, String> {
    let mut parsed: Vec<(&'static EventInfo, u32)> = Vec::new();
    for item in value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
    {
        let (id, rounds) = match item.split_once(':') {
            Some((id, rounds)) => (
                id.trim(),
                parse_number(rounds.trim(), "GENERATED_EVENTS rounds", 1, 4)?,
            ),
            None => (item, 1),
        };
        let event = events::find(id)
            .ok_or_else(|| format!("GENERATED_EVENTS has an unknown event {:?}", id))?;
        if parsed.iter().any(|(other, _)| other.id == event.id) {
            return Err(format!("GENERATED_EVENTS lists {:?} twice", id));
        }
        parsed.push((event, rounds));
    }
    if parsed.is_empty() {
        return Err("GENERATED_EVENTS must list at least one event".to_string());
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn config(vars: &[(&str, &str)]) -> Result<GeneratorConfig, String> {
        let vars: HashMap<String, String> = vars
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        GeneratorConfig::from_vars(|key| vars.get(key).cloned())
    }

    #[test]
    fn defaults_are_valid() {
        let config = GeneratorConfig::default();
        assert_eq!(config.days, 2);
        assert_eq!(config.competitors, 60);
        assert_eq!(config.events[0].0.id, "333");
        assert_eq!(config.events[0].1, 3);
        assert_eq!(config.host.country_iso2, "PL");
        assert!(config.start_date.is_none());
    }

    #[test]
    fn parses_every_setting() {
        let config = config(&[
            ("GENERATED_NAME", "Spring Test Open"),
            ("GENERATED_START_DATE", "2027-03-20"),
            ("GENERATED_DAYS", "3"),
            ("GENERATED_COMPETITORS", "120"),
            ("GENERATED_EVENTS", "333:4, 333fm, 333mbf:1"),
            ("GENERATED_ROOMS", "1"),
            ("GENERATED_TIMEZONE", "America/New_York"),
            ("GENERATED_SEED", "42"),
        ])
        .unwrap();
        assert_eq!(config.name, "Spring Test Open");
        assert_eq!(config.start_date, NaiveDate::from_ymd_opt(2027, 3, 20));
        assert_eq!(config.events.len(), 3);
        assert_eq!((config.events[1].0.id, config.events[1].1), ("333fm", 1));
        assert_eq!(config.host.country_iso2, "US");
        assert_eq!(config.seed, 42);
    }

    #[test]
    fn rejects_bad_values() {
        assert!(config(&[("GENERATED_DAYS", "0")]).is_err());
        assert!(config(&[("GENERATED_COMPETITORS", "abc")]).is_err());
        assert!(config(&[("GENERATED_EVENTS", "333,444x")]).is_err());
        assert!(config(&[("GENERATED_EVENTS", "333,333")]).is_err());
        assert!(config(&[("GENERATED_EVENTS", "333:5")]).is_err());
        assert!(config(&[("GENERATED_START_DATE", "2026-02-30")]).is_err());
        assert!(config(&[("GENERATED_TIMEZONE", "Mars/Olympus")]).is_err());
        assert!(config(&[("GENERATED_TIMEZONE", "Africa/Nairobi")]).is_err());
    }

    #[test]
    fn today_is_taken_in_the_venue_timezone() {
        let config = GeneratorConfig::default();
        // 23:30 UTC on 26 Oct is already 27 Oct in Warsaw (UTC+1).
        let now = "2026-10-26T23:30:00Z".parse().unwrap();
        assert_eq!(
            config.start_date_at(now),
            NaiveDate::from_ymd_opt(2026, 10, 27).unwrap()
        );
    }
}
