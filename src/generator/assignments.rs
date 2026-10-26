use chrono::NaiveDateTime;

use super::config::GeneratorConfig;
use super::model::{Activity, Assignment, Event, Person, Venue};
use super::rng::Rng;

struct Group<'a> {
    activity: &'a Activity,
    number: u32,
}

fn overlaps(a: (NaiveDateTime, NaiveDateTime), b: (NaiveDateTime, NaiveDateTime)) -> bool {
    a.0 < b.1 && b.0 < a.1
}

/// Assigns round 1 groups like Groupifier would: competitors sorted by PB with
/// the fastest in the last group, then judges, scramblers and runners picked
/// from people who are free at that time. Later rounds stay unassigned until
/// results are in, as at a real competition.
pub fn assign(config: &GeneratorConfig, persons: &mut [Person], events: &[Event], venue: &Venue) {
    let mut rng = Rng::derive(config.seed, "assignments");
    let mut busy: Vec<Vec<(NaiveDateTime, NaiveDateTime)>> = vec![Vec::new(); persons.len()];
    let mut staff_count = vec![0u32; persons.len()];
    let mut staffed_groups: Vec<(&Activity, usize)> = Vec::new();

    for event in events {
        let round = &event.rounds[0];
        let mut groups: Vec<Group> = venue
            .rooms
            .iter()
            .flat_map(|room| room.activities.iter())
            .filter(|activity| activity.code == round.id)
            .flat_map(|activity| activity.children.iter())
            .map(|activity| Group {
                activity,
                number: activity
                    .code
                    .rsplit_once("-g")
                    .and_then(|(_, number)| number.parse().ok())
                    .expect("group codes end in -g<number>"),
            })
            .collect();
        groups.sort_by_key(|group| group.number);
        if groups.is_empty() {
            continue;
        }

        let ranking_kind = if event.info.ranks_by_average() {
            "average"
        } else {
            "single"
        };
        let mut competitors: Vec<usize> = (0..persons.len())
            .filter(|&i| persons[i].is_accepted() && persons[i].is_registered_for(event.info.id))
            .collect();
        // Slowest (or no result) first, so the fastest end up in the last group.
        competitors.sort_by_key(|&i| {
            let person = &persons[i];
            let best = person
                .personal_best(event.info.id, ranking_kind)
                .or_else(|| person.personal_best(event.info.id, "single"))
                .unwrap_or(i64::MAX);
            (std::cmp::Reverse(best), person.registrant_id)
        });

        let base = competitors.len() / groups.len();
        let extra = competitors.len() % groups.len();
        let mut next = 0;
        for (index, group) in groups.iter().enumerate() {
            let size = base + usize::from(index < extra);
            for (seat, &person) in competitors[next..next + size].iter().enumerate() {
                persons[person].assignments.push(Assignment {
                    activity_id: group.activity.id,
                    station_number: Some(seat as u32 + 1),
                    assignment_code: "competitor",
                });
                busy[person].push((group.activity.start, group.activity.end));
            }
            next += size;
            if !event.info.is_fewest_moves() {
                staffed_groups.push((group.activity, size));
            }
        }
    }

    let stations = venue.rooms[0].stations as usize;
    staffed_groups.sort_by_key(|(activity, _)| activity.id);
    for (activity, size) in staffed_groups {
        let slot = (activity.start, activity.end);
        let mut candidates: Vec<usize> = (0..persons.len())
            .filter(|&i| {
                persons[i].is_accepted()
                    && persons[i].wca_id.is_some()
                    && !busy[i].iter().any(|&other| overlaps(slot, other))
            })
            .collect();
        rng.shuffle(&mut candidates);
        candidates.sort_by_key(|&i| staff_count[i]);

        let scramblers = if stations >= 6 { 2 } else { 1 };
        let runners = if stations >= 8 { 2 } else { 1 };
        let tasks = std::iter::repeat_n("staff-scrambler", scramblers)
            .chain(std::iter::repeat_n("staff-runner", runners))
            .chain(std::iter::repeat_n("staff-judge", size.min(stations)));
        for (task, person) in tasks.zip(candidates) {
            persons[person].assignments.push(Assignment {
                activity_id: activity.id,
                station_number: None,
                assignment_code: task,
            });
            busy[person].push(slot);
            staff_count[person] += 1;
        }
    }
}
