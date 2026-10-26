use chrono::{Duration, NaiveDate, NaiveDateTime};

use super::config::GeneratorConfig;
use super::events::EventInfo;
use super::model::{Activity, Event, Room, Venue};

/// Minutes after local midnight.
pub const DAY_START: i64 = 9 * 60;
const NOMINAL_END: i64 = 19 * 60;
/// No activity may end later than this, local time.
pub const HARD_END: i64 = 23 * 60 + 30;
const LUNCH_AFTER: i64 = 12 * 60 + 30;
const LUNCH_MINUTES: i64 = 45;
const CHECKIN_MINUTES: i64 = 30;
const AWARDS_MINUTES: i64 = 15;

const ROOMS: [(&str, &str); 4] = [
    ("Main Hall", "#304a96"),
    ("Side Hall", "#e2a13c"),
    ("Blue Room", "#2f7d4f"),
    ("Green Room", "#a23b72"),
];

/// How long one group of a round takes: scrambling, attempts, judging.
fn group_minutes(info: &EventInfo) -> i64 {
    if info.is_fewest_moves() {
        return 3 * 60 + 15;
    }
    if info.is_multi_blind() {
        return 65;
    }
    let typical = if info.ranks_by_average() {
        info.median_average
    } else {
        info.median_single
    };
    let seconds = typical as f64 / 100.0;
    let minutes = 5.0 + info.attempts() as f64 * (seconds * 1.3 + 45.0) / 60.0;
    round_up_to_5(minutes).max(10)
}

fn round_up_to_5(minutes: f64) -> i64 {
    ((minutes / 5.0).ceil() as i64) * 5
}

fn at(start_date: NaiveDate, day: i64, minutes: i64) -> NaiveDateTime {
    (start_date + Duration::days(day))
        .and_hms_opt(0, 0, 0)
        .expect("midnight exists")
        + Duration::minutes(minutes)
}

struct Placement {
    event: usize,
    round: usize,
    group_minutes: i64,
    rooms_used: u32,
    duration: i64,
}

/// Lays out every round on one timeline: a round runs in as many rooms as it
/// has groups (up to all rooms), and the next round starts when it ends.
/// Round 1 of every event comes first, then the second rounds, and so on.
pub fn build(
    config: &GeneratorConfig,
    start_date: NaiveDate,
    events: &[Event],
    stations: u32,
) -> Venue {
    let mut placements: Vec<Placement> = Vec::new();
    for (event_index, event) in events.iter().enumerate() {
        for (round_index, round) in event.rounds.iter().enumerate() {
            let rooms_used = if event.info.is_fewest_moves() {
                1
            } else {
                config.rooms.min(round.groups)
            };
            let minutes = group_minutes(event.info);
            placements.push(Placement {
                event: event_index,
                round: round_index,
                group_minutes: minutes,
                rooms_used,
                duration: round.groups.div_ceil(rooms_used) as i64 * minutes,
            });
        }
    }
    placements.sort_by_key(|placement| (placement.round, placement.event));

    // Shrink groups when everything wouldn't fit in the competition days.
    let days = config.days as i64;
    let capacity = days * (NOMINAL_END - DAY_START - LUNCH_MINUTES) - CHECKIN_MINUTES;
    let total: i64 = placements.iter().map(|placement| placement.duration).sum();
    if total * 10 > capacity * 9 {
        let scale = capacity as f64 * 0.9 / total as f64;
        for placement in placements.iter_mut() {
            placement.group_minutes = round_up_to_5(placement.group_minutes as f64 * scale).max(5);
            let groups = events[placement.event].rounds[placement.round].groups;
            placement.duration =
                groups.div_ceil(placement.rooms_used) as i64 * placement.group_minutes;
        }
    }
    let total: i64 = placements.iter().map(|placement| placement.duration).sum();
    let per_day = (total + days - 1) / days;

    let mut rooms: Vec<Room> = (0..config.rooms as usize)
        .map(|index| Room {
            id: index as u32 + 1,
            name: ROOMS[index].0,
            color: ROOMS[index].1,
            stations,
            activities: Vec::new(),
        })
        .collect();

    let other = |name: &str, code: &str, day: i64, from: i64, minutes: i64| Activity {
        id: 0,
        name: name.to_string(),
        code: code.to_string(),
        start: at(start_date, day, from),
        end: at(start_date, day, from + minutes),
        children: Vec::new(),
    };

    let mut day = 0;
    let mut time = DAY_START;
    let mut used_today = 0;
    let mut had_lunch = false;
    rooms[0].activities.push(other(
        "Check-in",
        "other-checkin",
        day,
        time,
        CHECKIN_MINUTES,
    ));
    time += CHECKIN_MINUTES;

    for placement in &placements {
        if day + 1 < days && (time + placement.duration > NOMINAL_END || used_today >= per_day) {
            day += 1;
            time = DAY_START;
            used_today = 0;
            had_lunch = false;
        }
        if !had_lunch && time >= LUNCH_AFTER {
            rooms[0]
                .activities
                .push(other("Lunch", "other-lunch", day, time, LUNCH_MINUTES));
            time += LUNCH_MINUTES;
            had_lunch = true;
        }

        let event = &events[placement.event];
        let round = &event.rounds[placement.round];
        let round_name = format!("{}, Round {}", event.info.name, round.number);
        for room_index in 0..placement.rooms_used {
            let group_numbers: Vec<u32> = (room_index + 1..=round.groups)
                .step_by(placement.rooms_used as usize)
                .collect();
            let children: Vec<Activity> = group_numbers
                .iter()
                .enumerate()
                .map(|(wave, number)| {
                    let from = time + wave as i64 * placement.group_minutes;
                    Activity {
                        id: 0,
                        name: format!("{}, Group {}", round_name, number),
                        code: format!("{}-g{}", round.id, number),
                        start: at(start_date, day, from),
                        end: at(start_date, day, from + placement.group_minutes),
                        children: Vec::new(),
                    }
                })
                .collect();
            let minutes = group_numbers.len() as i64 * placement.group_minutes;
            rooms[room_index as usize].activities.push(Activity {
                id: 0,
                name: round_name.clone(),
                code: round.id.clone(),
                start: at(start_date, day, time),
                end: at(start_date, day, time + minutes),
                children,
            });
        }
        time += placement.duration;
        used_today += placement.duration;
    }
    rooms[0]
        .activities
        .push(other("Awards", "other-awards", day, time, AWARDS_MINUTES));

    number_activities(&mut rooms);
    Venue {
        name: "Dummy Community Hall".to_string(),
        rooms,
    }
}

/// Top-level activities get ids 1.. in time order, then their groups.
fn number_activities(rooms: &mut [Room]) {
    let mut order: Vec<(NaiveDateTime, usize, usize)> = Vec::new();
    for (room_index, room) in rooms.iter().enumerate() {
        for (activity_index, activity) in room.activities.iter().enumerate() {
            order.push((activity.start, room_index, activity_index));
        }
    }
    order.sort();

    let mut next_id = 1;
    for &(_, room, activity) in &order {
        rooms[room].activities[activity].id = next_id;
        next_id += 1;
    }
    for &(_, room, activity) in &order {
        for child in rooms[room].activities[activity].children.iter_mut() {
            child.id = next_id;
            next_id += 1;
        }
    }
}
