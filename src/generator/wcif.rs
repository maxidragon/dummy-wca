//! Renders the generated competition in the shapes the WCA API returns.

use chrono::{Duration, NaiveDateTime, TimeZone, Utc};
use serde_json::{json, Value};

use super::model::{Activity, Competition, Event, Person};
use super::names::{self, Account, ACCOUNTS};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Version {
    V1,
    V2,
}

const MISSING_AVATAR: &str = "https://www.worldcubeassociation.org/assets/missing_avatar_thumb.png";
const ACCOUNT_CREATED_AT: &str = "2020-01-01T00:00:00.000Z";

/// A local venue time as a WCIF UTC timestamp.
pub fn utc(competition: &Competition, local: NaiveDateTime) -> String {
    let zoned = competition
        .timezone
        .from_local_datetime(&local)
        .earliest()
        // Only in a DST gap, which the schedule never uses.
        .unwrap_or_else(|| {
            competition
                .timezone
                .from_local_datetime(&(local + Duration::hours(1)))
                .earliest()
                .expect("an hour after a DST gap exists")
        });
    zoned
        .with_timezone(&Utc)
        .format("%Y-%m-%dT%H:%M:%SZ")
        .to_string()
}

fn millis(timestamp: &str) -> String {
    timestamp.replace('Z', ".000Z")
}

/// The private WCIF, as a delegate or organizer sees it.
pub fn wcif(competition: &Competition, version: Version) -> Value {
    json!({
        "formatVersion": match version { Version::V1 => "1.1", Version::V2 => "2.1.1" },
        "id": competition.id,
        "name": competition.name,
        "shortName": competition.name,
        "series": null,
        "persons": competition.persons.iter().map(|person| person_json(person, version)).collect::<Vec<_>>(),
        "events": competition.events.iter().map(|event| event_json(event, version)).collect::<Vec<_>>(),
        "schedule": {
            "startDate": competition.start_date.format("%Y-%m-%d").to_string(),
            "numberOfDays": competition.number_of_days,
            "venues": [{
                "id": 1,
                "name": competition.venue.name,
                "latitudeMicrodegrees": competition.host.latitude_microdegrees,
                "longitudeMicrodegrees": competition.host.longitude_microdegrees,
                "countryIso2": competition.host.country_iso2,
                "timezone": competition.timezone.name(),
                "rooms": competition.venue.rooms.iter().map(|room| json!({
                    "id": room.id,
                    "name": room.name,
                    "color": room.color,
                    "activities": room.activities.iter().map(|activity| activity_json(competition, activity)).collect::<Vec<_>>(),
                    "extensions": [{
                        "id": "groupifier.RoomConfig",
                        "specUrl": "https://groupifier.jonatanklosko.com/wcif-extensions/RoomConfig.json",
                        "data": { "stations": room.stations },
                    }],
                })).collect::<Vec<_>>(),
                "extensions": [],
            }],
        },
        "competitorLimit": competition.competitor_limit,
        "extensions": [],
        "registrationInfo": {
            "openTime": utc(competition, competition.registration_open),
            "closeTime": utc(competition, competition.registration_close),
            "baseEntryFee": competition.host.base_entry_fee,
            "currencyCode": competition.host.currency_code,
            "onTheSpotRegistration": false,
            "useWcaRegistration": true,
        },
    })
}

/// What anyone can see: accepted competitors only, without contact details.
/// Works on any WCIF (generated or snapshot), after patches are applied.
pub fn public_view(mut wcif: Value) -> Value {
    if let Some(persons) = wcif["persons"].as_array_mut() {
        persons.retain(|person| person["registration"]["status"] == "accepted");
        for person in persons.iter_mut() {
            if let Some(person) = person.as_object_mut() {
                person.remove("birthdate");
                person.remove("email");
            }
        }
    }
    wcif
}

fn person_json(person: &Person, version: Version) -> Value {
    let value_key = match version {
        Version::V1 => "best",
        Version::V2 => "value",
    };
    json!({
        "name": person.name,
        "wcaUserId": person.wca_user_id,
        "wcaId": person.wca_id,
        "registrantId": person.registrant_id,
        "countryIso2": person.country_iso2,
        "gender": person.gender,
        "birthdate": person.birthdate.format("%Y-%m-%d").to_string(),
        "email": person.email,
        "registration": {
            "wcaRegistrationId": person.registration.wca_registration_id,
            "eventIds": person.registration.event_ids,
            "status": person.registration.status,
            "isCompeting": person.registration.is_competing,
        },
        "avatar": { "url": MISSING_AVATAR, "thumbUrl": MISSING_AVATAR },
        "roles": person.roles,
        "assignments": person.assignments.iter().map(|assignment| json!({
            "activityId": assignment.activity_id,
            "stationNumber": assignment.station_number,
            "assignmentCode": assignment.assignment_code,
        })).collect::<Vec<_>>(),
        "personalBests": person.personal_bests.iter().map(|pb| json!({
            "eventId": pb.event_id,
            (value_key): pb.value,
            "worldRanking": pb.world_ranking,
            "continentalRanking": pb.continental_ranking,
            "nationalRanking": pb.national_ranking,
            "type": pb.kind,
        })).collect::<Vec<_>>(),
        "extensions": [],
    })
}

fn event_json(event: &Event, version: Version) -> Value {
    let info = event.info;
    let scope = if info.ranks_by_average() {
        "average"
    } else {
        "single"
    };
    let rounds: Vec<Value> = event
        .rounds
        .iter()
        .enumerate()
        .map(|(index, round)| {
            let time_limit = info.time_limit.map(|centiseconds| {
                json!({
                    "centiseconds": centiseconds,
                    "cumulativeRoundIds": if info.cumulative_limit { vec![round.id.clone()] } else { vec![] },
                })
            });
            let mut value = json!({
                "id": round.id,
                "format": info.format,
                "timeLimit": time_limit,
                "scrambleSetCount": round.groups,
                "results": [],
                "extensions": [],
            });
            let result_key = match version {
                Version::V1 => "attemptResult",
                Version::V2 => "resultValue",
            };
            value["cutoff"] = match round.cutoff {
                Some((attempts, result)) => json!({ "numberOfAttempts": attempts, (result_key): result }),
                None => Value::Null,
            };
            match version {
                Version::V1 => {
                    value["advancementCondition"] = match round.advancement {
                        Some(advancement) => json!({ "type": advancement.kind, "level": advancement.level }),
                        None => Value::Null,
                    };
                }
                Version::V2 => {
                    value["linkedRounds"] = Value::Null;
                    let source = match index.checked_sub(1).map(|previous| &event.rounds[previous]) {
                        None => json!({ "type": "registrations" }),
                        Some(previous) => {
                            let advancement = previous.advancement.expect("every round but the final has one");
                            json!({
                                "type": "round",
                                "roundId": previous.id,
                                "resultCondition": {
                                    "type": advancement.kind,
                                    "scope": scope,
                                    "value": advancement.level,
                                },
                            })
                        }
                    };
                    value["participationRuleset"] = json!({ "participationSource": source, "reservedPlaces": null });
                }
            }
            value
        })
        .collect();

    let mut value = json!({ "id": info.id, "rounds": rounds, "extensions": [] });
    value["qualification"] = Value::Null;
    value
}

fn activity_json(competition: &Competition, activity: &Activity) -> Value {
    json!({
        "id": activity.id,
        "name": activity.name,
        "activityCode": activity.code,
        "startTime": utc(competition, activity.start),
        "endTime": utc(competition, activity.end),
        "childActivities": activity.children.iter().map(|child| activity_json(competition, child)).collect::<Vec<_>>(),
        "extensions": [],
    })
}

fn country_json(iso2: &str) -> Value {
    let country = names::country(iso2);
    json!({
        "id": country.name,
        "name": country.name,
        "continentId": country.continent_id,
        "iso2": country.iso2,
    })
}

fn avatar_json() -> Value {
    json!({ "url": MISSING_AVATAR, "thumb_url": MISSING_AVATAR, "is_default": true })
}

struct UserFields<'a> {
    id: u64,
    name: &'a str,
    wca_id: Option<&'a str>,
    gender: &'a str,
    country_iso2: &'a str,
    delegate_status: Option<&'a str>,
    team: Option<&'a str>,
}

fn user_json(user: UserFields) -> Value {
    let teams: Vec<Value> = user
        .team
        .map(|team| {
            json!({
                "id": user.id,
                "friendly_id": team,
                "leader": false,
                "senior_member": false,
                "name": user.name,
                "wca_id": user.wca_id,
                "avatar": avatar_json(),
            })
        })
        .into_iter()
        .collect();
    json!({
        "id": user.id,
        "created_at": ACCOUNT_CREATED_AT,
        "updated_at": ACCOUNT_CREATED_AT,
        "name": user.name,
        "wca_id": user.wca_id,
        "gender": user.gender,
        "country_iso2": user.country_iso2,
        "url": user.wca_id.map(|id| format!("https://www.worldcubeassociation.org/persons/{}", id)),
        "country": country_json(user.country_iso2),
        "delegate_status": user.delegate_status,
        "class": "user",
        "teams": teams,
        "avatar": avatar_json(),
    })
}

pub fn account_user(account: &Account, competition: &Competition) -> Value {
    user_json(UserFields {
        id: account.user_id,
        name: account.name,
        wca_id: Some(account.wca_id),
        gender: account.gender,
        country_iso2: competition.host.country_iso2,
        delegate_status: account.delegate_status,
        team: account.team,
    })
}

fn person_user(person: &Person) -> Value {
    user_json(UserFields {
        id: person.wca_user_id,
        name: &person.name,
        wca_id: person.wca_id.as_deref(),
        gender: person.gender,
        country_iso2: person.country_iso2,
        delegate_status: None,
        team: None,
    })
}

/// The user behind a WCA ID: a fixed account or a generated person.
pub fn user_by_wca_id(competition: &Competition, wca_id: &str) -> Option<Value> {
    if let Some(account) = names::account(wca_id) {
        return Some(account_user(account, competition));
    }
    competition.person_by_wca_id(wca_id).map(person_user)
}

/// Everyone with a WCA ID, in the `/api/v0/persons` shape.
pub fn person_entries(competition: &Competition) -> Vec<Value> {
    let accounts = ACCOUNTS
        .iter()
        .map(|account| account_user(account, competition));
    let persons = competition
        .persons
        .iter()
        .filter(|person| {
            person.wca_id.is_some()
                && names::account(person.wca_id.as_deref().unwrap_or_default()).is_none()
        })
        .map(person_user);
    accounts
        .chain(persons)
        .map(|mut user| {
            user["class"] = json!("person");
            json!({
                "person": user,
                "competition_count": 1,
                "personal_records": {},
                "medals": { "gold": 0, "silver": 0, "bronze": 0, "total": 0 },
                "records": { "national": 0, "continental": 0, "world": 0, "total": 0 },
            })
        })
        .collect()
}

fn staff(competition: &Competition, role: &str) -> Vec<Value> {
    competition
        .persons
        .iter()
        .filter(|person| person.roles.contains(&role))
        .filter_map(|person| person.wca_id.as_deref().and_then(names::account))
        .map(|account| account_user(account, competition))
        .collect()
}

pub fn manager_role(competition: &Competition, wca_id: &str) -> Option<&'static str> {
    competition.person_by_wca_id(wca_id).and_then(|person| {
        person
            .roles
            .iter()
            .copied()
            .find(|role| *role == "delegate" || *role == "organizer")
    })
}

/// One item of `GET /api/v0/competitions?managed_by_me=true`.
pub fn listing(competition: &Competition) -> Value {
    json!({
        "id": competition.id,
        "name": competition.name,
        "url": format!("http://localhost:3000/competitions/{}", competition.id),
        "registration_open": millis(&utc(competition, competition.registration_open)),
        "start_date": competition.start_date.format("%Y-%m-%d").to_string(),
        "end_date": competition.end_date().format("%Y-%m-%d").to_string(),
        "country_iso2": competition.host.country_iso2,
        "city": "Dummy City",
        "class": "competition",
        "delegates": staff(competition, "delegate"),
        "organizers": staff(competition, "organizer"),
    })
}

/// `GET /api/v0/competitions/:id`.
pub fn competition_info(competition: &Competition) -> Value {
    let open = millis(&utc(competition, competition.registration_open));
    let close = millis(&utc(competition, competition.registration_close));
    json!({
        "id": competition.id,
        "name": competition.name,
        "short_name": competition.name,
        "information": "A generated test competition. Every person and result in it is invented.",
        "venue": competition.venue.name,
        "venue_address": "1 Example Street, Dummy City",
        "venue_details": "",
        "city": "Dummy City",
        "country_iso2": competition.host.country_iso2,
        "latitude_degrees": competition.host.latitude_microdegrees as f64 / 1e6,
        "longitude_degrees": competition.host.longitude_microdegrees as f64 / 1e6,
        "start_date": competition.start_date.format("%Y-%m-%d").to_string(),
        "end_date": competition.end_date().format("%Y-%m-%d").to_string(),
        "registration_open": open,
        "registration_close": close,
        "announced_at": open,
        "cancelled_at": null,
        "competitor_limit": competition.competitor_limit,
        "use_wca_registration": true,
        "on_the_spot_registration": false,
        "base_entry_fee_lowest_denomination": competition.host.base_entry_fee,
        "currency_code": competition.host.currency_code,
        "event_ids": competition.events.iter().map(|event| event.info.id).collect::<Vec<_>>(),
        "main_event_id": competition.events[0].info.id,
        "url": format!("http://localhost:3000/competitions/{}", competition.id),
        "website": format!("http://localhost:3000/competitions/{}", competition.id),
        "delegates": staff(competition, "delegate"),
        "organizers": staff(competition, "organizer"),
        "class": "competition",
    })
}
