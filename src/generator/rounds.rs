use super::config::GeneratorConfig;
use super::model::{Advancement, Event, Person, Round, HEAD_TO_HEAD};

/// Stations per room: enough that round 1 of a popular event takes a few groups.
pub fn stations(accepted: u32, rooms: u32) -> u32 {
    accepted.div_ceil(rooms * 4).clamp(4, 20)
}

/// Most rounds an event can have for this many competitors (WCA Regulation 9m).
fn max_rounds(competitors: u32) -> u32 {
    match competitors {
        0..=7 => 1,
        8..=15 => 2,
        16..=99 => 3,
        _ => 4,
    }
}

pub fn build(config: &GeneratorConfig, persons: &[Person], stations: u32) -> Vec<Event> {
    config
        .events
        .iter()
        .map(|&(info, requested)| {
            let registered = persons
                .iter()
                .filter(|person| person.is_accepted() && person.is_registered_for(info.id))
                .count() as u32;
            let round_count = requested.min(max_rounds(registered));
            // The final is head to head only when a round before it seeds the
            // bracket (I1b); dummy-wca doesn't generate seeding without one (I1c).
            let head_to_head_final = info.has_head_to_head_final() && round_count >= 2;

            let mut sizes = vec![registered];
            let mut advancements = Vec::new();
            for number in 2..=round_count {
                let previous = *sizes.last().expect("round 1 is always there");
                let is_final = number == round_count;
                let advancement = if number == 2 && round_count >= 3 {
                    Advancement {
                        kind: "percent",
                        level: 75,
                    }
                } else if is_final {
                    let cap = if info.popularity >= 0.5 { 16 } else { 12 };
                    let level = (previous * 3 / 4).min(cap);
                    Advancement {
                        kind: "ranking",
                        level: if head_to_head_final {
                            bracket_size(level)
                        } else {
                            level
                        },
                    }
                } else {
                    Advancement {
                        kind: "ranking",
                        level: previous * 3 / 4,
                    }
                };
                let advancing = match advancement.kind {
                    "percent" => previous * advancement.level / 100,
                    _ => advancement.level,
                };
                advancements.push(advancement);
                sizes.push(advancing);
            }

            let rounds = sizes
                .iter()
                .enumerate()
                .map(|(index, &competitors)| {
                    let number = index as u32 + 1;
                    let head_to_head = head_to_head_final && number == round_count;
                    // A head-to-head round is one group in one room (SPEC-001).
                    let groups = if info.is_fewest_moves() || head_to_head {
                        1
                    } else {
                        competitors.div_ceil(stations).max(1)
                    };
                    Round {
                        id: format!("{}-r{}", info.id, number),
                        number,
                        format: if head_to_head {
                            HEAD_TO_HEAD
                        } else {
                            info.format
                        },
                        competitors,
                        groups,
                        cutoff: if number == 1 { info.cutoff } else { None },
                        advancement: advancements.get(index).copied(),
                    }
                })
                .collect();
            Event { info, rounds }
        })
        .collect()
}

/// The largest head-to-head bracket (Article I: 4, 8, 12 or 16) for at most
/// `competitors`. A final always has at least 6, so at least 4 fit.
fn bracket_size(competitors: u32) -> u32 {
    [16, 12, 8, 4]
        .into_iter()
        .find(|size| *size <= competitors)
        .expect("a final has at least 6 competitors")
}

#[cfg(test)]
mod tests {
    use super::bracket_size;

    #[test]
    fn brackets_take_the_largest_size_that_fits() {
        for (competitors, size) in [
            (6, 4),
            (7, 4),
            (8, 8),
            (11, 8),
            (12, 12),
            (15, 12),
            (16, 16),
        ] {
            assert_eq!(bracket_size(competitors), size, "{}", competitors);
        }
    }
}
