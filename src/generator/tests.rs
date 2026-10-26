use std::collections::{HashMap, HashSet};
use std::time::Instant;

use chrono::{Datelike, NaiveDate, NaiveTime, Timelike};
use serde_json::{json, Value};

use super::events;
use super::model::Activity;
use super::wcif::{self, Version};
use super::*;

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn config(vars: &[(&str, &str)]) -> GeneratorConfig {
    let vars: HashMap<String, String> = vars
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    GeneratorConfig::from_vars(|key| vars.get(key).cloned()).unwrap()
}

fn is_wca_id(id: &str) -> bool {
    id.len() == 10
        && id[..4].chars().all(|c| c.is_ascii_digit())
        && id[4..8].chars().all(|c| c.is_ascii_uppercase())
        && id[8..].chars().all(|c| c.is_ascii_digit())
}

#[test]
fn same_config_gives_identical_wcif() {
    let config = GeneratorConfig::default();
    let a = wcif::wcif(&generate(&config, date("2026-10-26")), Version::V2);
    let b = wcif::wcif(&generate(&config, date("2026-10-26")), Version::V2);
    assert_eq!(a, b);
}

#[test]
fn another_seed_gives_other_people() {
    let a = generate(&GeneratorConfig::default(), date("2026-10-26"));
    let b = generate(&config(&[("GENERATED_SEED", "2")]), date("2026-10-26"));
    let names = |c: &Competition| c.persons.iter().map(|p| p.name.clone()).collect::<Vec<_>>();
    assert_ne!(names(&a), names(&b));
}

#[test]
fn moving_the_date_only_moves_dates() {
    let config = GeneratorConfig::default();
    let a = wcif::wcif(&generate(&config, date("2026-10-26")), Version::V2);
    let b = wcif::wcif(&generate(&config, date("2026-11-30")), Version::V2);
    assert_eq!(a["persons"], b["persons"]);
    assert_eq!(a["events"], b["events"]);
    assert_eq!(a["id"], b["id"]);
    assert_ne!(a["schedule"], b["schedule"]);
}

#[test]
fn changing_events_keeps_the_same_people() {
    let a = generate(&GeneratorConfig::default(), date("2026-10-26"));
    let b = generate(
        &config(&[("GENERATED_EVENTS", "333,pyram")]),
        date("2026-10-26"),
    );
    let ids = |c: &Competition| {
        c.persons
            .iter()
            .map(|p| (p.name.clone(), p.wca_id.clone()))
            .collect::<Vec<_>>()
    };
    assert_eq!(ids(&a), ids(&b));
}

#[test]
fn year_follows_the_start_date() {
    let config = GeneratorConfig::default();
    let december = generate(&config, date("2026-12-31"));
    let january = generate(&config, date("2027-01-01"));
    assert_eq!(december.id, "DummyCubingOpen2026");
    assert_eq!(december.name, "Dummy Cubing Open 2026");
    assert_eq!(january.id, "DummyCubingOpen2027");
    assert_eq!(
        competition_id("Spring-Test Open!", 2030),
        "SpringTestOpen2030"
    );
}

#[test]
fn people_look_like_wca_people() {
    let competition = generate(
        &config(&[("GENERATED_COMPETITORS", "300")]),
        date("2026-10-26"),
    );
    assert_eq!(competition.persons.len(), 300);

    let mut wca_ids = HashSet::new();
    let mut user_ids = HashSet::new();
    let mut newcomers = 0;
    for person in &competition.persons {
        assert!(user_ids.insert(person.wca_user_id));
        assert!(["m", "f", "o"].contains(&person.gender));
        assert!(
            person
                .birthdate
                .format("%Y")
                .to_string()
                .parse::<i32>()
                .unwrap()
                <= 2018
        );
        assert!(!person.registration.event_ids.is_empty());
        match &person.wca_id {
            Some(id) => {
                assert!(is_wca_id(id), "{}", id);
                assert!(wca_ids.insert(id.clone()), "duplicate {}", id);
            }
            None => {
                newcomers += 1;
                assert!(person.personal_bests.is_empty());
            }
        }
        assert_eq!(person.registrant_id.is_some(), person.is_accepted());
    }
    // About 15% newcomers and 90% accepted.
    assert!((20..=70).contains(&newcomers), "{} newcomers", newcomers);
    let accepted = competition.accepted().count();
    assert!((240..=290).contains(&accepted), "{} accepted", accepted);
    assert!(competition.competitor_limit as usize >= accepted);

    // Registrant ids are 1..=accepted.
    let mut registrant_ids: Vec<u32> = competition
        .persons
        .iter()
        .filter_map(|p| p.registrant_id)
        .collect();
    registrant_ids.sort();
    assert_eq!(registrant_ids, (1..=accepted as u32).collect::<Vec<_>>());
}

#[test]
fn fixed_accounts_run_the_competition() {
    let competition = generate(&GeneratorConfig::default(), date("2026-10-26"));
    assert_eq!(
        wcif::manager_role(&competition, "2012VAQR01"),
        Some("delegate")
    );
    assert_eq!(
        wcif::manager_role(&competition, "2015yqvi01"),
        Some("organizer")
    );
    assert_eq!(wcif::manager_role(&competition, "2017KEXW01"), None);
    assert_eq!(wcif::manager_role(&competition, "2011ULQA01"), None);
    for account in names::ACCOUNTS.iter() {
        assert!(is_wca_id(account.wca_id));
    }
    // Nobody started competing before they were born.
    for person in &competition.persons {
        if let Some(wca_id) = &person.wca_id {
            let first_year: i32 = wca_id[..4].parse().unwrap();
            assert!(
                first_year >= person.birthdate.year() + 6,
                "{} born {}",
                wca_id,
                person.birthdate
            );
        }
    }
}

#[test]
fn personal_bests_are_in_sensible_ranges() {
    let competition = generate(
        &config(&[("GENERATED_COMPETITORS", "400")]),
        date("2026-10-26"),
    );
    let mut seen = 0;
    for person in &competition.persons {
        for pb in &person.personal_bests {
            seen += 1;
            let event = events::find(pb.event_id).unwrap();
            assert!(pb.world_ranking >= 1 && pb.national_ranking <= pb.world_ranking);
            if event.is_multi_blind() {
                let points = 99 - pb.value / 10_000_000;
                let seconds = pb.value / 100 % 100_000;
                assert!(points >= 1 && seconds <= 3600, "{}", pb.value);
                continue;
            }
            let (floor, ceiling) = if event.is_fewest_moves() {
                match pb.kind {
                    "single" => (event.floor_single as i64, 80),
                    _ => (event.floor_average as i64, 8100),
                }
            } else {
                match pb.kind {
                    "single" => (event.floor_single as i64, event.median_single as i64 * 4),
                    _ => (event.floor_average as i64, event.median_average as i64 * 4),
                }
            };
            assert!(
                (floor..=ceiling).contains(&pb.value),
                "{} {} {}",
                pb.event_id,
                pb.kind,
                pb.value
            );
        }
        for event in events::EVENTS.iter().filter(|e| !e.is_multi_blind()) {
            if let (Some(single), Some(average)) = (
                person.personal_best(event.id, "single"),
                person.personal_best(event.id, "average"),
            ) {
                let single = if event.is_fewest_moves() {
                    single * 100
                } else {
                    single
                };
                assert!(single <= average, "{} {} > {}", event.id, single, average);
            }
        }
    }
    assert!(seen > 1000);
}

fn all_activities(competition: &Competition) -> Vec<&Activity> {
    competition.activities()
}

#[test]
fn schedule_is_consistent() {
    for vars in [
        vec![],
        vec![("GENERATED_ROOMS", "1"), ("GENERATED_DAYS", "1"), ("GENERATED_COMPETITORS", "30")],
        vec![("GENERATED_ROOMS", "3"), ("GENERATED_DAYS", "3"), ("GENERATED_COMPETITORS", "250"),
             ("GENERATED_EVENTS", "333:4,222:3,444:3,555:2,666,777,333bf:2,333fm,333oh:2,clock:2,minx,pyram:3,skewb:2,sq1:2,444bf,555bf,333mbf")],
    ] {
        let config = config(&vars);
        let competition = generate(&config, date("2026-10-26"));
        check(&config, competition.start_date).unwrap();

        // Unique ids; groups nested in their round.
        let activities = all_activities(&competition);
        let ids: HashSet<u32> = activities.iter().map(|a| a.id).collect();
        assert_eq!(ids.len(), activities.len());
        for room in &competition.venue.rooms {
            let mut sorted: Vec<&Activity> = room.activities.iter().collect();
            sorted.sort_by_key(|a| a.start);
            for pair in sorted.windows(2) {
                assert!(pair[0].end <= pair[1].start, "{} overlaps {}", pair[0].code, pair[1].code);
            }
            for activity in &room.activities {
                assert!(activity.start < activity.end);
                assert!(activity.start.date() >= competition.start_date);
                assert!(activity.end.date() <= competition.end_date());
                for child in &activity.children {
                    assert!(child.code.starts_with(&format!("{}-g", activity.code)));
                    assert!(activity.start <= child.start && child.end <= activity.end);
                }
            }
        }

        // Every round is scheduled, after the previous one, with as many groups as scramble sets.
        for event in &competition.events {
            let mut previous_end = None;
            for round in &event.rounds {
                let activities: Vec<&&Activity> = activities.iter().filter(|a| a.code == round.id).collect();
                assert!(!activities.is_empty(), "{} not scheduled", round.id);
                let groups: usize = activities.iter().map(|a| a.children.len()).sum();
                assert_eq!(groups as u32, round.groups, "{}", round.id);
                let start = activities.iter().map(|a| a.start).min().unwrap();
                if let Some(end) = previous_end {
                    assert!(start >= end, "{} starts before the previous round ends", round.id);
                }
                previous_end = activities.iter().map(|a| a.end).max();
                if let Some(advancement) = round.advancement {
                    assert!(advancement.kind == "percent" && advancement.level <= 75
                        || advancement.kind == "ranking" && advancement.level <= round.competitors * 3 / 4);
                }
            }
            assert!(event.rounds.last().unwrap().advancement.is_none());
        }
    }
}

#[test]
fn rounds_follow_wca_limits() {
    let competition = generate(
        &config(&[
            ("GENERATED_COMPETITORS", "12"),
            ("GENERATED_EVENTS", "333:4,444:3"),
        ]),
        date("2026-10-26"),
    );
    for event in &competition.events {
        let first = &event.rounds[0];
        let max = match first.competitors {
            0..=7 => 1,
            8..=15 => 2,
            16..=99 => 3,
            _ => 4,
        };
        assert!(
            event.rounds.len() <= max,
            "{} has {} rounds for {}",
            event.info.id,
            event.rounds.len(),
            first.competitors
        );
    }
}

#[test]
fn assignments_are_consistent() {
    let competition = generate(&GeneratorConfig::default(), date("2026-10-26"));
    let activities: HashMap<u32, &Activity> = all_activities(&competition)
        .into_iter()
        .map(|a| (a.id, a))
        .collect();
    let mut staffed = 0;
    for person in &competition.persons {
        let mut slots = Vec::new();
        for assignment in &person.assignments {
            let activity = activities
                .get(&assignment.activity_id)
                .expect("assignment points at a real activity");
            assert!(activity.code.contains("-g"), "{}", activity.code);
            assert!(person.is_accepted());
            if assignment.assignment_code == "competitor" {
                let event_id = activity.code.split('-').next().unwrap();
                assert!(person.is_registered_for(event_id));
                assert!(assignment.station_number.is_some());
            } else {
                staffed += 1;
                assert!(
                    activity.code.contains("-r1-g"),
                    "staff only in round 1: {}",
                    activity.code
                );
                assert!(person.wca_id.is_some(), "newcomers don't staff");
            }
            slots.push((activity.start, activity.end));
        }
        slots.sort();
        for pair in slots.windows(2) {
            assert!(
                pair[0].1 <= pair[1].0,
                "{} is in two places at once",
                person.name
            );
        }
    }
    assert!(staffed > 50);

    // Later rounds have exactly the expected number of competitors.
    for event in &competition.events {
        for round in event.rounds.iter().skip(1) {
            let group_ids: HashSet<u32> = activities
                .values()
                .filter(|a| a.code.starts_with(&format!("{}-g", round.id)))
                .map(|a| a.id)
                .collect();
            let assigned = competition
                .persons
                .iter()
                .filter(|p| {
                    p.assignments.iter().any(|a| {
                        a.assignment_code == "competitor" && group_ids.contains(&a.activity_id)
                    })
                })
                .count();
            assert_eq!(assigned as u32, round.competitors, "{}", round.id);
        }
    }

    // Every accepted competitor has one competitor assignment per round-1 event.
    for event in &competition.events {
        let group_ids: HashSet<u32> = activities
            .values()
            .filter(|a| a.code.starts_with(&format!("{}-r1-g", event.info.id)))
            .map(|a| a.id)
            .collect();
        for person in competition
            .accepted()
            .filter(|p| p.is_registered_for(event.info.id))
        {
            let count = person
                .assignments
                .iter()
                .filter(|a| a.assignment_code == "competitor" && group_ids.contains(&a.activity_id))
                .count();
            assert_eq!(count, 1, "{} in {}", person.name, event.info.id);
        }
    }
}

#[test]
fn local_times_hold_across_dst() {
    // Europe/Warsaw leaves summer time on 25 Oct 2026.
    let config = config(&[("GENERATED_DAYS", "3")]);
    let competition = generate(&config, date("2026-10-24"));
    let wcif = wcif::wcif(&competition, Version::V2);
    let rooms = wcif["schedule"]["venues"][0]["rooms"].as_array().unwrap();
    let mut first_per_day: HashMap<String, String> = HashMap::new();
    for room in rooms {
        for activity in room["activities"].as_array().unwrap() {
            let start = activity["startTime"].as_str().unwrap().to_string();
            let day = start[..10].to_string();
            let entry = first_per_day.entry(day).or_insert(start.clone());
            if start < *entry {
                *entry = start;
            }
        }
    }
    assert_eq!(first_per_day["2026-10-24"], "2026-10-24T07:00:00Z");
    assert_eq!(first_per_day["2026-10-25"], "2026-10-25T08:00:00Z");
    assert_eq!(first_per_day["2026-10-26"], "2026-10-26T08:00:00Z");

    for activity in competition.activities() {
        assert!(activity.start.time() >= NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    }
    let first_day_start = competition
        .activities()
        .iter()
        .map(|a| a.start)
        .min()
        .unwrap();
    assert_eq!(first_day_start.hour(), 9);

    // Registration times use the venue's offset on their own dates (summer time).
    assert_eq!(wcif["registrationInfo"]["openTime"], "2026-09-26T17:00:00Z");
    assert_eq!(
        wcif["registrationInfo"]["closeTime"],
        "2026-10-17T21:59:00Z"
    );
}

#[test]
fn southern_hemisphere_dst_too() {
    // Australia/Sydney starts summer time on 4 Oct 2026.
    let config = config(&[
        ("GENERATED_DAYS", "2"),
        ("GENERATED_TIMEZONE", "Australia/Sydney"),
    ]);
    let wcif = wcif::wcif(&generate(&config, date("2026-10-03")), Version::V2);
    let starts: Vec<String> = wcif["schedule"]["venues"][0]["rooms"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|room| room["activities"].as_array().unwrap().iter())
        .map(|activity| activity["startTime"].as_str().unwrap().to_string())
        .collect();
    // 09:00 AEST is 23:00Z the day before; 09:00 AEDT is 22:00Z the day before.
    assert!(starts.contains(&"2026-10-02T23:00:00Z".to_string()));
    assert!(starts.contains(&"2026-10-03T22:00:00Z".to_string()));
}

#[test]
fn wcif_versions_have_the_expected_shapes() {
    let competition = generate(&GeneratorConfig::default(), date("2026-10-26"));
    let v1 = wcif::wcif(&competition, Version::V1);
    let v2 = wcif::wcif(&competition, Version::V2);
    assert_eq!(v1["formatVersion"], "1.1");
    assert_eq!(v2["formatVersion"], "2.1.1");

    let with_pbs = |w: &Value| {
        w["persons"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| !p["personalBests"].as_array().unwrap().is_empty())
            .unwrap()
            .clone()
    };
    assert!(with_pbs(&v1)["personalBests"][0].get("best").is_some());
    assert!(with_pbs(&v2)["personalBests"][0].get("value").is_some());

    let event = |w: &Value, id: &str| {
        w["events"]
            .as_array()
            .unwrap()
            .iter()
            .find(|e| e["id"] == id)
            .unwrap()
            .clone()
    };
    let v1_444 = event(&v1, "444");
    let v2_444 = event(&v2, "444");
    assert_eq!(v1_444["rounds"][0]["cutoff"]["attemptResult"], 12000);
    assert_eq!(v2_444["rounds"][0]["cutoff"]["resultValue"], 12000);
    assert_eq!(
        v1_444["rounds"][0]["advancementCondition"]["type"],
        "ranking"
    );
    assert!(v2_444["rounds"][0].get("advancementCondition").is_none());
    assert_eq!(
        v2_444["rounds"][0]["participationRuleset"]["participationSource"]["type"],
        "registrations"
    );
    let source = &v2_444["rounds"][1]["participationRuleset"]["participationSource"];
    assert_eq!(source["type"], "round");
    assert_eq!(source["roundId"], "444-r1");
    assert_eq!(source["resultCondition"]["scope"], "average");

    let v2_333 = event(&v2, "333");
    assert_eq!(
        v2_333["rounds"][1]["participationRuleset"]["participationSource"]["resultCondition"]
            ["type"],
        "percent"
    );
    assert_eq!(
        event(&v2, "333bf")["rounds"][0]["timeLimit"]["cumulativeRoundIds"][0],
        "333bf-r1"
    );

    for key in [
        "id",
        "name",
        "shortName",
        "persons",
        "events",
        "schedule",
        "competitorLimit",
        "extensions",
        "registrationInfo",
    ] {
        assert!(v1.get(key).is_some() && v2.get(key).is_some(), "{}", key);
    }
}

#[test]
fn public_view_hides_private_data() {
    let competition = generate(&GeneratorConfig::default(), date("2026-10-26"));
    let private = wcif::wcif(&competition, Version::V2);
    let public = wcif::public_view(private.clone());
    let persons = public["persons"].as_array().unwrap();
    assert_eq!(persons.len(), competition.accepted().count());
    assert!(private["persons"].as_array().unwrap().len() > persons.len());
    for person in persons {
        assert!(person.get("birthdate").is_none() && person.get("email").is_none());
        assert_eq!(person["registration"]["status"], "accepted");
    }
}

#[test]
fn generating_is_quick() {
    let config = config(&[("GENERATED_COMPETITORS", "500"), ("GENERATED_DAYS", "3")]);
    let started = Instant::now();
    let competition = generate(&config, date("2026-10-26"));
    let _ = wcif::wcif(&competition, Version::V2);
    let elapsed = started.elapsed();
    // The server is built in release mode; debug builds are a few times slower.
    let budget = if cfg!(debug_assertions) { 250 } else { 50 };
    println!("generated 500 competitors in {:?}", elapsed);
    assert!(elapsed.as_millis() < budget, "took {:?}", elapsed);
}

#[test]
fn oversized_configurations_are_rejected() {
    let config = config(&[
        ("GENERATED_COMPETITORS", "1000"),
        ("GENERATED_DAYS", "1"),
        ("GENERATED_ROOMS", "1"),
        ("GENERATED_EVENTS", "333:4,222:4,444:4,555:4,666:3,777:3,333bf:3,333fm:3,333oh:4,clock:4,minx:4,pyram:4,skewb:4,sq1:4"),
    ]);
    assert!(check(&config, date("2026-10-26")).is_err());
}

/// FKMTime's tests run against the default competition, so it has to keep
/// every WCIF feature they exercise. Each check names what depends on it.
#[test]
fn default_competition_covers_what_fkmtime_uses() {
    let config = GeneratorConfig::default();
    let competition = generate(&config, date("2026-10-26"));
    check(&config, competition.start_date).unwrap();
    let wcif = wcif::wcif(&competition, Version::V2);

    let rounds: Vec<&Value> = wcif["events"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|event| event["rounds"].as_array().unwrap())
        .collect();
    let has_round = |what: &str, test: &dyn Fn(&Value) -> bool| {
        assert!(
            rounds.iter().any(|round| test(round)),
            "no round with {}",
            what
        );
    };

    // Cutoffs are read as `resultValue` only.
    for round in &rounds {
        assert!(round["cutoff"].get("attemptResult").is_none());
    }
    has_round("Ao5, time limit, no cutoff", &|r| {
        r["format"] == "a" && r["timeLimit"]["centiseconds"].is_u64() && r["cutoff"].is_null()
    });
    has_round("Mo3 with a 1-attempt cutoff", &|r| {
        r["format"] == "m"
            && r["cutoff"]["numberOfAttempts"] == 1
            && r["cutoff"]["resultValue"].is_u64()
    });
    has_round("Ao5 with a 2-attempt cutoff", &|r| {
        r["format"] == "a"
            && r["cutoff"]["numberOfAttempts"] == 2
            && r["cutoff"]["resultValue"].is_u64()
    });
    has_round("a cumulative limit over itself", &|r| {
        r["timeLimit"]["cumulativeRoundIds"] == json!([r["id"]])
    });
    has_round("Bo5", &|r| r["format"] == "5");
    has_round("Bo3", &|r| r["format"] == "3");

    // An event with 3 rounds and advancement through participationRuleset.
    let three_rounds = wcif["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|event| event["rounds"].as_array().unwrap().len() >= 3)
        .expect("an event with 3 rounds");
    let source = |index: usize| {
        three_rounds["rounds"][index]["participationRuleset"]["participationSource"].clone()
    };
    assert_eq!(source(0)["type"], "registrations");
    assert_eq!(source(1)["type"], "round");
    assert_eq!(source(1)["resultCondition"]["type"], "percent");
    assert_eq!(source(2)["type"], "round");
    assert_eq!(source(2)["resultCondition"]["type"], "ranking");

    // One venue with two rooms running the same round at the same time, in
    // several groups whose codes are unique across rooms.
    let venues = wcif["schedule"]["venues"].as_array().unwrap();
    assert_eq!(venues.len(), 1);
    let rooms = venues[0]["rooms"].as_array().unwrap();
    assert!(rooms.len() >= 2);
    let activities = |room: &Value| room["activities"].as_array().unwrap().clone();
    let parallel = activities(&rooms[0]).iter().any(|a| {
        activities(&rooms[1]).iter().any(|b| {
            a["activityCode"] == b["activityCode"]
                && a["startTime"].as_str() < b["endTime"].as_str()
                && b["startTime"].as_str() < a["endTime"].as_str()
        })
    });
    assert!(parallel, "no round runs in two rooms at once");
    assert!(rooms
        .iter()
        .flat_map(&activities)
        .any(|a| a["childActivities"].as_array().unwrap().len() >= 2));
    let group_codes: Vec<String> = rooms
        .iter()
        .flat_map(&activities)
        .flat_map(|a| a["childActivities"].as_array().unwrap().clone())
        .map(|g| g["activityCode"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        group_codes.len(),
        group_codes.iter().collect::<HashSet<_>>().len()
    );
    assert!(rooms.iter().flat_map(&activities).any(|a| {
        a["activityCode"] == "other-checkin" && a["childActivities"].as_array().unwrap().is_empty()
    }));

    // Activities on both days.
    assert_eq!(wcif["schedule"]["numberOfDays"], 2);
    for day in 0..2 {
        let day = competition.start_date + chrono::Duration::days(day);
        assert!(
            competition
                .activities()
                .iter()
                .any(|a| a.start.date() == day),
            "nothing on {}",
            day
        );
    }

    // Assignments: every code FKMTime maps, and competitors beyond round 1.
    let persons = wcif["persons"].as_array().unwrap();
    let codes: HashSet<&str> = persons
        .iter()
        .flat_map(|p| p["assignments"].as_array().unwrap())
        .map(|a| a["assignmentCode"].as_str().unwrap())
        .collect();
    for code in [
        "competitor",
        "staff-judge",
        "staff-scrambler",
        "staff-runner",
    ] {
        assert!(codes.contains(code), "no {} assignment", code);
    }
    let later_groups: HashSet<u32> = competition
        .activities()
        .iter()
        .filter(|a| a.code.contains("-r2-g"))
        .map(|a| a.id)
        .collect();
    assert!(persons
        .iter()
        .flat_map(|p| p["assignments"].as_array().unwrap())
        .any(|a| {
            a["assignmentCode"] == "competitor"
                && later_groups.contains(&(a["activityId"].as_u64().unwrap() as u32))
        }));

    // Registrations sync drops, newcomers, and someone skipping an event.
    for status in ["accepted", "pending", "deleted"] {
        assert!(
            persons
                .iter()
                .any(|p| p["registration"]["status"] == status),
            "no {} registration",
            status
        );
    }
    assert!(persons.iter().any(|p| {
        p["wcaId"].is_null()
            && p["registration"]["status"] == "accepted"
            && p["birthdate"].is_string()
    }));
    let event_ids: Vec<&str> = competition.events.iter().map(|e| e.info.id).collect();
    assert!(competition
        .accepted()
        .any(|p| event_ids.iter().any(|id| !p.is_registered_for(id))));

    // Station messages depend on judge and competitor sharing a country.
    let judges: Vec<&Value> = persons
        .iter()
        .filter(|p| {
            p["assignments"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a["assignmentCode"] == "staff-judge")
        })
        .collect();
    assert!(judges.iter().any(|p| p["countryIso2"] == "PL"));
    assert!(judges.iter().any(|p| p["countryIso2"] != "PL"));
}
