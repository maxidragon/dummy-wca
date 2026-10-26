use std::collections::HashMap;

use chrono::{Datelike, NaiveDate};

use super::config::GeneratorConfig;
use super::events::{self, EventInfo, EVENTS};
use super::model::{Person, PersonalBest, Registration};
use super::names::{self, ACCOUNTS};
use super::rng::Rng;

/// Ages and first-competition years are relative to this year, not the start
/// date, so people stay the same whatever date the competition is on.
const REFERENCE_YEAR: i32 = 2026;

const FIRST_GENERATED_USER_ID: u64 = 9_100_000;

/// People, without registrations or results. Index 0 and 1 are the fixed
/// delegate and organizer accounts.
pub fn people(config: &GeneratorConfig) -> Vec<Person> {
    let mut rng = Rng::derive(config.seed, "persons");
    let mut serials: HashMap<String, u32> = HashMap::new();
    let mut persons = Vec::with_capacity(config.competitors as usize);

    for index in 0..config.competitors as usize {
        let account = ACCOUNTS
            .iter()
            .filter(|account| account.role.is_some())
            .nth(index);
        let gender: &'static str = match account {
            Some(account) => account.gender,
            None => rng.weighted(&[("m", 70), ("f", 27), ("o", 3)]),
        };
        let age = 8 + (rng.unit() * rng.unit() * 50.0) as i32;
        let birth_year = REFERENCE_YEAR - age;
        let birthdate =
            NaiveDate::from_ymd_opt(birth_year, rng.range(1, 12) as u32, rng.range(1, 28) as u32)
                .expect("day 1-28 exists in every month");
        let first_year = rng.range(
            (birth_year + 7).max(2005) as i64,
            (REFERENCE_YEAR - 1) as i64,
        ) as i32;
        let is_newcomer = rng.chance(0.15);
        let status = *rng.weighted(&[("accepted", 90), ("pending", 6), ("deleted", 4)]);
        let skill = rng.normal();

        let person = match account {
            Some(account) => Person {
                name: account.name.to_string(),
                wca_user_id: account.user_id,
                wca_id: Some(account.wca_id.to_string()),
                registrant_id: None,
                country_iso2: config.host.country_iso2,
                gender,
                // Adults who started competing at 18, matching their WCA ID year.
                birthdate: birthdate
                    .with_year(account.wca_id[..4].parse::<i32>().expect("WCA IDs start with a year") - 18)
                    .expect("day 1-28 exists in every year"),
                email: format!("{}@dummy-wca.invalid", account.user_id),
                registration: registration(index, "accepted"),
                roles: vec![account.role.expect("filtered on role")],
                assignments: Vec::new(),
                personal_bests: Vec::new(),
                skill,
            },
            None => {
                let surname = names::surname(&mut rng);
                let name = format!("{} {}", names::first_name(&mut rng, gender), surname);
                let wca_id = (!is_newcomer).then(|| {
                    let prefix = format!("{}{}", first_year, surname[..4].to_ascii_uppercase());
                    let serial = serials.entry(prefix.clone()).or_insert(0);
                    *serial += 1;
                    format!("{}{:02}", prefix, serial)
                });
                let wca_user_id = FIRST_GENERATED_USER_ID + index as u64;
                Person {
                    name,
                    wca_user_id,
                    wca_id,
                    registrant_id: None,
                    country_iso2: names::country_iso2(&mut rng, config.host.country_iso2),
                    gender,
                    birthdate,
                    email: format!("{}@dummy-wca.invalid", wca_user_id),
                    registration: registration(index, status),
                    roles: Vec::new(),
                    assignments: Vec::new(),
                    personal_bests: Vec::new(),
                    skill,
                }
            }
        };
        persons.push(person);
    }

    for (registrant_id, person) in
        (1..).zip(persons.iter_mut().filter(|person| person.is_accepted()))
    {
        person.registrant_id = Some(registrant_id);
    }

    // A couple of data-entry volunteers, like most competitions have.
    let mut volunteers: Vec<usize> = (2..persons.len())
        .filter(|&i| persons[i].is_accepted() && persons[i].wca_id.is_some())
        .collect();
    rng.shuffle(&mut volunteers);
    for &i in volunteers.iter().take(2) {
        persons[i].roles.push("staff-dataentry");
    }

    persons
}

fn registration(index: usize, status: &'static str) -> Registration {
    Registration {
        wca_registration_id: 7_000_000 + index as u64,
        event_ids: Vec::new(),
        status,
        is_competing: status != "deleted",
    }
}

/// Picks events for everyone, weighted by how popular each event is, and makes
/// sure every event has at least two accepted competitors.
pub fn register(config: &GeneratorConfig, persons: &mut [Person]) {
    let mut rng = Rng::derive(config.seed, "registrations");
    for person in persons.iter_mut() {
        for (event, _) in &config.events {
            if rng.chance(event.popularity) {
                person.registration.event_ids.push(event.id);
            }
        }
        if person.registration.event_ids.is_empty() {
            person.registration.event_ids.push(config.events[0].0.id);
        }
    }

    let accepted: Vec<usize> = (0..persons.len())
        .filter(|&i| persons[i].is_accepted())
        .collect();
    for (event, _) in &config.events {
        let wanted = accepted.len().min(2);
        let mut missing = wanted.saturating_sub(
            accepted
                .iter()
                .filter(|&&i| persons[i].is_registered_for(event.id))
                .count(),
        );
        let mut candidates = accepted.clone();
        rng.shuffle(&mut candidates);
        for i in candidates {
            if missing == 0 {
                break;
            }
            if !persons[i].is_registered_for(event.id) {
                persons[i].registration.event_ids.push(event.id);
                missing -= 1;
            }
        }
    }

    let order = |id: &str| config.events.iter().position(|(event, _)| event.id == id);
    for person in persons.iter_mut() {
        person.registration.event_ids.sort_by_key(|id| order(id));
    }
}

/// Personal bests for everyone with a WCA ID: most of the events they
/// registered for, plus some others they've done before.
pub fn personal_bests(config: &GeneratorConfig, persons: &mut [Person]) {
    let mut rng = Rng::derive(config.seed, "results");
    for person in persons.iter_mut() {
        if person.wca_id.is_none() {
            continue;
        }
        for event in EVENTS.iter() {
            let has_result = if person.is_registered_for(event.id) {
                rng.chance(0.9)
            } else {
                rng.chance(event.popularity * 0.4)
            };
            if !has_result {
                continue;
            }
            let ability = 0.8 * person.skill + 0.6 * rng.normal();
            for (kind, value) in results(&mut rng, event, ability) {
                let ranked = if kind == "single" {
                    event.ranked_people
                } else {
                    event.ranked_people * 4 / 5
                };
                // Faster people rank higher; a rough linear stand-in for the normal CDF.
                let share = (0.5 - ability / 6.0).clamp(0.0005, 0.9995);
                let world = ((share * ranked as f64) as u32).max(1);
                person.personal_bests.push(PersonalBest {
                    event_id: event.id,
                    kind,
                    value,
                    world_ranking: world,
                    continental_ranking: (world * 3 / 10).max(1),
                    national_ranking: (world / 25).max(1),
                });
            }
        }
    }
}

/// How much slower than the median someone with this ability is.
/// Uses only arithmetic so results are the same on every platform.
fn slowdown(ability: f64) -> f64 {
    if ability >= 0.0 {
        1.0 / (1.0 + 0.35 * ability)
    } else {
        1.0 - 0.35 * ability
    }
}

fn results(rng: &mut Rng, event: &EventInfo, ability: f64) -> Vec<(&'static str, i64)> {
    let factor = slowdown(ability);
    if event.is_multi_blind() {
        let attempted = (4.0 + 2.0 * ability).round().clamp(2.0, 16.0) as u32;
        let solved = attempted - rng.range(0, (attempted as i64 - 1) / 2) as u32;
        let seconds = ((attempted * 600).min(3600) as f64 * (0.6 + 0.35 * rng.unit())) as u32;
        return vec![(
            "single",
            events::encode_multi_blind(solved, attempted, seconds),
        )];
    }
    if event.is_fewest_moves() {
        let single = (event.median_single as f64 * factor)
            .round()
            .clamp(event.floor_single as f64, 80.0) as i64;
        let mut found = vec![("single", single)];
        if rng.chance(event.average_share) {
            let average_moves =
                (single as f64 * (1.05 + 0.1 * rng.unit())).max(event.floor_average as f64 / 100.0);
            let total_moves = (average_moves * 3.0).round() as i64;
            found.push(("average", (total_moves * 100 + 1) / 3));
        }
        return found;
    }

    let average = ((event.median_average as f64 * factor) as i64).max(event.floor_average as i64);
    let single = ((event.median_single as f64 * factor * (0.9 + 0.2 * rng.unit())) as i64)
        .max(event.floor_single as i64)
        .min(average);
    let mut found = vec![("single", single)];
    if rng.chance(event.average_share) {
        found.push(("average", average));
    }
    found
}
