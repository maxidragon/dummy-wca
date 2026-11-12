use chrono::Utc;
use serde_json::json;

use super::*;

/// An Ao5 event with two rounds; the top `proceed` of round 1 go on.
fn definition(condition: serde_json::Value) -> Definition {
    Definition::from_wcif(&json!({
        "persons": (1..=12).map(|id| json!({
            "name": format!("Person {}", id),
            "wcaUserId": 100 + id,
            "registrantId": id,
            "countryIso2": "XX",
            "registration": {
                "wcaRegistrationId": id,
                "eventIds": ["333"],
                "status": "accepted",
                "isCompeting": true,
            },
        })).collect::<Vec<_>>(),
        "events": [{
            "id": "333",
            "rounds": [
                {
                    "id": "333-r1",
                    "format": "a",
                    "cutoff": null,
                    "results": [],
                    "participationRuleset": { "participationSource": { "type": "registrations" } },
                },
                {
                    "id": "333-r2",
                    "format": "a",
                    "cutoff": null,
                    "results": [],
                    "participationRuleset": { "participationSource": {
                        "type": "round",
                        "roundId": "333-r1",
                        "resultCondition": condition,
                    } },
                },
            ],
        }],
    }))
}

fn ranking(value: i64) -> serde_json::Value {
    json!({ "type": "ranking", "scope": "average", "value": value })
}

/// A complete Ao5 result with this average and best.
fn result(registration_id: u64, average: i64, best: i64) -> LiveResult {
    LiveResult {
        attempts: (1..=5)
            .map(|attempt_number| Attempt {
                attempt_number,
                value: best,
            })
            .collect(),
        best,
        average,
        ..LiveResult::empty(registration_id, registration_id, Utc::now())
    }
}

fn condition(definition: &Definition) -> Option<Condition> {
    match definition.rounds[1].source {
        Source::Round(_, condition) => condition,
        Source::Registrations => None,
    }
}

fn advancing(results: &[LiveResult]) -> Vec<u64> {
    results
        .iter()
        .filter(|result| result.advancing)
        .map(|result| result.registration_id)
        .collect()
}

#[test]
fn reads_rounds_and_registrations_from_the_wcif() {
    let definition = definition(ranking(3));
    assert_eq!(definition.rounds.len(), 2);
    let round = &definition.rounds[1];
    assert_eq!(round.wcif_id, "333-r2");
    assert_eq!((round.number, round.total_rounds), (2, 2));
    assert_eq!(round.source, Source::Round(0, Some(Condition::Ranking(3))));
    assert!(round.wcif.get("results").is_none());
    assert!(
        round.wcif.get("cutoff").is_none(),
        "blank values are dropped"
    );
    assert_eq!(definition.next_round(0), Some(1));
    assert_eq!(definition.registrations.len(), 12);
}

#[test]
fn ties_share_a_position() {
    let definition = definition(ranking(3));
    let round = &definition.rounds[0];
    let mut results = vec![
        result(1, 1200, 1000),
        result(2, 1100, 1000),
        result(3, 1200, 1000),
        LiveResult::empty(4, 4, Utc::now()),
        result(5, -1, 900),
    ];
    round.recompute(condition(&definition), &mut results, false);
    let positions: Vec<Option<u32>> = results.iter().map(|result| result.local_pos).collect();
    assert_eq!(positions, [Some(2), Some(1), Some(2), None, Some(4)]);
}

#[test]
fn the_top_proceed_but_never_more_than_75_percent() {
    let definition = definition(ranking(3));
    let round = &definition.rounds[0];
    let mut results: Vec<LiveResult> = (1..=8)
        .map(|id| result(id, 1000 + id as i64, 900))
        .collect();
    round.recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1, 2, 3]);
    assert!(results[2].advancing_questionable);
    assert!(!results[3].advancing_questionable);

    let definition = self::definition(ranking(10));
    let mut results: Vec<LiveResult> = (1..=4)
        .map(|id| result(id, 1000 + id as i64, 900))
        .collect();
    definition.rounds[0].recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1, 2, 3], "75% of 4");
}

#[test]
fn ties_at_the_boundary_all_proceed_unless_too_many() {
    let definition = definition(ranking(2));
    let round = &definition.rounds[0];
    let mut results = vec![
        result(1, 1000, 900),
        result(2, 1100, 900),
        result(3, 1100, 900),
        result(4, 1300, 900),
        result(5, 1400, 900),
    ];
    round.recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1, 2, 3]);

    // 75% of 4 is 3: a three-way tie for second would make four.
    let mut results = vec![
        result(1, 1000, 900),
        result(2, 1100, 900),
        result(3, 1100, 900),
        result(4, 1100, 900),
    ];
    round.recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1]);
}

#[test]
fn incomplete_results_hold_a_place_but_do_not_proceed() {
    let definition = definition(ranking(2));
    let round = &definition.rounds[0];
    let mut partial = result(1, 0, 800);
    partial.attempts.truncate(2);
    let mut results = vec![
        partial,
        result(2, 1000, 900),
        result(3, 1100, 900),
        result(4, 1200, 900),
    ];
    round.recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [2], "1 might still beat 2");
}

#[test]
fn percent_and_result_achieved_conditions() {
    let definition = definition(json!({ "type": "percent", "scope": "average", "value": 50 }));
    let mut results: Vec<LiveResult> = (1..=6)
        .map(|id| result(id, 1000 + id as i64, 900))
        .collect();
    definition.rounds[0].recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1, 2, 3]);

    let definition =
        self::definition(json!({ "type": "resultAchieved", "scope": "average", "value": 1003 }));
    definition.rounds[0].recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1, 2]);
}

#[test]
fn a_final_marks_the_podium() {
    let definition = definition(ranking(3));
    let mut results: Vec<LiveResult> = (1..=5)
        .map(|id| result(id, 1000 + id as i64, 900))
        .collect();
    definition.rounds[1].recompute(None, &mut results, false);
    assert_eq!(advancing(&results), [1, 2, 3]);
}

#[test]
fn locked_results_keep_whether_they_proceed() {
    let definition = definition(ranking(1));
    let round = &definition.rounds[0];
    let mut results = vec![result(1, 1000, 900), result(2, 1100, 900)];
    round.recompute(condition(&definition), &mut results, false);
    assert_eq!(advancing(&results), [1]);
    for result in results.iter_mut() {
        result.locked = true;
    }
    results[1].average = 900;
    round.recompute(condition(&definition), &mut results, true);
    assert_eq!(results[1].local_pos, Some(1), "positions still follow");
    assert_eq!(advancing(&results), [1], "advancement is frozen");
}

fn state(definition: &Definition, results: &[Vec<LiveResult>], index: usize) -> State {
    Rounds {
        definition,
        results: results.iter().map(Vec::as_slice).collect(),
    }
    .state(index)
}

#[test]
fn rounds_go_from_pending_to_locked() {
    let definition = definition(ranking(3));
    let empty = vec![Vec::new(), Vec::new()];
    assert_eq!(state(&definition, &empty, 0), State::Ready);
    assert_eq!(state(&definition, &empty, 1), State::Pending);

    let mut first: Vec<LiveResult> = (1..=9)
        .map(|id| LiveResult::empty(id, id, Utc::now()))
        .collect();
    let open = vec![first.clone(), Vec::new()];
    assert_eq!(state(&definition, &open, 0), State::Open);
    assert_eq!(state(&definition, &open, 1), State::Pending);

    for (index, slot) in first.iter_mut().enumerate() {
        *slot = result(slot.registration_id, 1000 + index as i64, 900);
    }
    let done = vec![first.clone(), Vec::new()];
    assert_eq!(state(&definition, &done, 1), State::Ready);

    for result in first.iter_mut() {
        result.locked = true;
    }
    let locked = vec![first, vec![LiveResult::empty(99, 1, Utc::now())]];
    assert_eq!(state(&definition, &locked, 0), State::Locked);
    assert_eq!(state(&definition, &locked, 1), State::Open);
}

#[test]
fn a_round_after_too_few_competitors_is_blocked() {
    let definition = definition(ranking(3));
    let few: Vec<LiveResult> = (1..=7).map(|id| result(id, 1000, 900)).collect();
    let results = vec![few, Vec::new()];
    assert_eq!(state(&definition, &results, 1), State::Blocked);
    let rounds = Rounds {
        definition: &definition,
        results: results.iter().map(Vec::as_slice).collect(),
    };
    assert_eq!(rounds.insufficient_competitors_violation(1), Some("9m3"));
    assert_eq!(rounds.info_json(1)["competitor_count_needed"], 8);
    assert_eq!(rounds.info_json(1)["min_competitors_to_open"], 8);
}

/// Round 1 done and locked with the top 3 proceeding.
fn after_round_one(averages: &[i64]) -> (Definition, Vec<LiveResult>) {
    let definition = definition(ranking(3));
    let mut results: Vec<LiveResult> = averages
        .iter()
        .enumerate()
        .map(|(index, average)| result(index as u64 + 1, *average, 900))
        .collect();
    definition.rounds[0].recompute(condition(&definition), &mut results, false);
    (definition, results)
}

fn next_without(definition: &Definition, results: &[LiveResult], quitting: u64) -> Vec<u64> {
    definition.rounds[1]
        .next_participating_without(&definition.rounds[0], results, &[quitting])
        .iter()
        .map(|result| result.registration_id)
        .collect()
}

#[test]
fn next_if_quit_names_who_would_proceed_instead() {
    let (definition, results) = after_round_one(&[1000, 1100, 1200, 1300, 1400, 1500]);
    assert_eq!(next_without(&definition, &results, 1), [4]);

    // Tied for fourth: both would proceed.
    let (definition, results) = after_round_one(&[1000, 1100, 1200, 1300, 1300, 1500]);
    assert_eq!(next_without(&definition, &results, 1), [4, 5]);

    // Nobody left who didn't proceed.
    let (definition, mut results) = after_round_one(&[1000, 1100, 1200]);
    for result in results.iter_mut() {
        result.advancing = true;
    }
    assert!(next_without(&definition, &results, 1).is_empty());
}

#[test]
fn next_if_quit_also_sets_aside_the_winner_like_the_wca() {
    // The WCA (after WCA Live) treats the top-placed competitor as gone too,
    // so someone else quitting frees two places.
    let (definition, results) = after_round_one(&[1000, 1100, 1200, 1300, 1400, 1500]);
    assert_eq!(next_without(&definition, &results, 2), [4, 5]);
}

#[test]
fn the_state_hash_follows_the_results() {
    let mut results = vec![LiveResult::empty(1, 1, Utc::now())];
    let before = Round::state_hash(&results);
    assert_eq!(before.len(), 40);
    assert_eq!(before, Round::state_hash(&results));
    results[0].best = 900;
    assert_ne!(before, Round::state_hash(&results));
}
