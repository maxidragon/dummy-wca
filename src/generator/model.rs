//! A format-neutral model of the generated competition. `wcif.rs` renders it
//! as WCIF v1 or v2.

use chrono::{NaiveDate, NaiveDateTime};
use chrono_tz::Tz;

use super::events::EventInfo;
use super::names::Host;

pub struct Competition {
    pub id: String,
    pub name: String,
    pub start_date: NaiveDate,
    pub number_of_days: u32,
    pub timezone: Tz,
    pub host: &'static Host,
    pub persons: Vec<Person>,
    pub events: Vec<Event>,
    pub venue: Venue,
    pub registration_open: NaiveDateTime,
    pub registration_close: NaiveDateTime,
    pub competitor_limit: u32,
}

impl Competition {
    pub fn end_date(&self) -> NaiveDate {
        self.start_date + chrono::Duration::days(self.number_of_days as i64 - 1)
    }

    pub fn person_by_wca_id(&self, wca_id: &str) -> Option<&Person> {
        self.persons.iter().find(|person| {
            person
                .wca_id
                .as_deref()
                .is_some_and(|id| id.eq_ignore_ascii_case(wca_id))
        })
    }

    pub fn accepted(&self) -> impl Iterator<Item = &Person> {
        self.persons.iter().filter(|person| person.is_accepted())
    }

    /// Every activity, top-level and child, in id order.
    pub fn activities(&self) -> Vec<&Activity> {
        let mut all: Vec<&Activity> = Vec::new();
        for room in &self.venue.rooms {
            for activity in &room.activities {
                all.push(activity);
                all.extend(activity.children.iter());
            }
        }
        all.sort_by_key(|activity| activity.id);
        all
    }
}

pub struct Person {
    pub name: String,
    pub wca_user_id: u64,
    pub wca_id: Option<String>,
    /// Only accepted registrations get a registrant id, as on the WCA.
    pub registrant_id: Option<u32>,
    pub country_iso2: &'static str,
    pub gender: &'static str,
    pub birthdate: NaiveDate,
    pub email: String,
    pub registration: Registration,
    pub roles: Vec<&'static str>,
    pub assignments: Vec<Assignment>,
    pub personal_bests: Vec<PersonalBest>,
    /// Latent ability (about standard normal); higher is faster.
    pub skill: f64,
}

impl Person {
    pub fn is_accepted(&self) -> bool {
        self.registration.status == "accepted"
    }

    pub fn is_registered_for(&self, event_id: &str) -> bool {
        self.registration.event_ids.contains(&event_id)
    }

    pub fn personal_best(&self, event_id: &str, kind: &str) -> Option<i64> {
        self.personal_bests
            .iter()
            .find(|pb| pb.event_id == event_id && pb.kind == kind)
            .map(|pb| pb.value)
    }
}

pub struct Registration {
    pub wca_registration_id: u64,
    pub event_ids: Vec<&'static str>,
    pub status: &'static str,
    pub is_competing: bool,
}

#[derive(Clone)]
pub struct Assignment {
    pub activity_id: u32,
    pub station_number: Option<u32>,
    pub assignment_code: &'static str,
}

pub struct PersonalBest {
    pub event_id: &'static str,
    /// `single` or `average`.
    pub kind: &'static str,
    pub value: i64,
    pub world_ranking: u32,
    pub continental_ranking: u32,
    pub national_ranking: u32,
}

pub struct Event {
    pub info: &'static EventInfo,
    pub rounds: Vec<Round>,
}

pub struct Round {
    pub id: String,
    pub number: u32,
    /// Expected number of competitors (registrations, or the advancing count).
    pub competitors: u32,
    pub groups: u32,
    pub cutoff: Option<(u32, u32)>,
    /// How competitors advance to the next round; `None` for the final.
    pub advancement: Option<Advancement>,
}

#[derive(Clone, Copy)]
pub struct Advancement {
    /// `ranking` or `percent`.
    pub kind: &'static str,
    pub level: u32,
}

pub struct Venue {
    pub name: String,
    pub rooms: Vec<Room>,
}

pub struct Room {
    pub id: u32,
    pub name: &'static str,
    pub color: &'static str,
    pub stations: u32,
    pub activities: Vec<Activity>,
}

/// Times are local wall-clock times in the venue timezone; `wcif.rs` converts
/// them to UTC. That keeps local times fixed across DST changes.
pub struct Activity {
    pub id: u32,
    pub name: String,
    pub code: String,
    pub start: NaiveDateTime,
    pub end: NaiveDateTime,
    pub children: Vec<Activity>,
}
