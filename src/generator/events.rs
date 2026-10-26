/// How an event is held and what results in it typically look like.
///
/// Times are in centiseconds. For `333fm` singles are moves and averages are
/// moves × 100, as in WCIF. `333mbf` results are encoded separately.
pub struct EventInfo {
    pub id: &'static str,
    pub name: &'static str,
    /// WCIF round format: `a` (Ao5), `m` (Mo3), `5` (Bo5), `3` (Bo3), `1` (Bo1).
    pub format: &'static str,
    /// `None` for events without a time limit in WCIF (`333fm`, `333mbf`).
    pub time_limit: Option<u32>,
    pub cumulative_limit: bool,
    /// Round 1 cutoff: number of attempts and result to beat.
    pub cutoff: Option<(u32, u32)>,
    pub median_single: u32,
    pub median_average: u32,
    /// Generated results never beat these (a little above the world records).
    pub floor_single: u32,
    pub floor_average: u32,
    /// Share of competitors with a PB in this event who also have an average.
    pub average_share: f64,
    /// Chance that a competitor registers for this event.
    pub popularity: f64,
    /// Number of people in the world with a result, for rankings.
    pub ranked_people: u32,
}

impl EventInfo {
    pub fn attempts(&self) -> u32 {
        match self.format {
            "a" | "5" => 5,
            "m" | "3" => 3,
            "2" => 2,
            _ => 1,
        }
    }

    pub fn is_fewest_moves(&self) -> bool {
        self.id == "333fm"
    }

    pub fn is_multi_blind(&self) -> bool {
        self.id == "333mbf"
    }

    /// Advancement and cutoffs compare averages for Ao5/Mo3, singles otherwise.
    pub fn ranks_by_average(&self) -> bool {
        matches!(self.format, "a" | "m")
    }
}

macro_rules! event {
    ($id:expr, $name:expr, $format:expr, $limit:expr, $cumulative:expr, $cutoff:expr,
     $single:expr, $average:expr, $floor_single:expr, $floor_average:expr,
     $average_share:expr, $popularity:expr, $ranked:expr) => {
        EventInfo {
            id: $id,
            name: $name,
            format: $format,
            time_limit: $limit,
            cumulative_limit: $cumulative,
            cutoff: $cutoff,
            median_single: $single,
            median_average: $average,
            floor_single: $floor_single,
            floor_average: $floor_average,
            average_share: $average_share,
            popularity: $popularity,
            ranked_people: $ranked,
        }
    };
}

/// All 17 current WCA events, in the WCA's order.
pub const EVENTS: [EventInfo; 17] = [
    event!(
        "333",
        "3x3x3 Cube",
        "a",
        Some(60000),
        false,
        None,
        1500,
        1900,
        330,
        450,
        1.0,
        0.95,
        250000
    ),
    event!(
        "222",
        "2x2x2 Cube",
        "a",
        Some(6000),
        false,
        None,
        450,
        620,
        60,
        100,
        1.0,
        0.7,
        200000
    ),
    event!(
        "444",
        "4x4x4 Cube",
        "a",
        Some(18000),
        false,
        Some((2, 12000)),
        5500,
        6300,
        1600,
        1950,
        0.95,
        0.5,
        90000
    ),
    event!(
        "555",
        "5x5x5 Cube",
        "a",
        Some(30000),
        false,
        Some((2, 18000)),
        10000,
        11200,
        3300,
        3650,
        0.9,
        0.3,
        50000
    ),
    event!(
        "666",
        "6x6x6 Cube",
        "m",
        Some(60000),
        false,
        Some((1, 30000)),
        20000,
        21500,
        6000,
        6700,
        0.85,
        0.15,
        20000
    ),
    event!(
        "777",
        "7x7x7 Cube",
        "m",
        Some(60000),
        false,
        Some((1, 42000)),
        30000,
        31500,
        9600,
        10000,
        0.85,
        0.15,
        15000
    ),
    event!(
        "333bf",
        "3x3x3 Blindfolded",
        // Bo5, as many competitions now hold it and FKMTime's tests use it.
        "5",
        Some(60000),
        true,
        None,
        12000,
        15000,
        1300,
        1600,
        0.3,
        0.2,
        30000
    ),
    event!(
        "333fm",
        "3x3x3 Fewest Moves",
        "m",
        None,
        false,
        None,
        38,
        4100,
        17,
        2100,
        0.5,
        0.15,
        25000
    ),
    event!(
        "333oh",
        "3x3x3 One-Handed",
        "a",
        Some(60000),
        false,
        None,
        3000,
        3700,
        620,
        870,
        1.0,
        0.45,
        80000
    ),
    event!(
        "clock",
        "Clock",
        "a",
        Some(6000),
        false,
        Some((2, 2000)),
        1300,
        1600,
        230,
        260,
        1.0,
        0.3,
        40000
    ),
    event!(
        "minx",
        "Megaminx",
        "a",
        Some(30000),
        false,
        Some((2, 18000)),
        9000,
        10500,
        2500,
        2800,
        0.95,
        0.25,
        35000
    ),
    event!(
        "pyram",
        "Pyraminx",
        "a",
        Some(6000),
        false,
        None,
        700,
        950,
        80,
        150,
        1.0,
        0.6,
        150000
    ),
    event!(
        "skewb",
        "Skewb",
        "a",
        Some(6000),
        false,
        None,
        800,
        1050,
        80,
        150,
        1.0,
        0.5,
        100000
    ),
    event!(
        "sq1",
        "Square-1",
        "a",
        Some(12000),
        false,
        Some((2, 6000)),
        3000,
        3800,
        340,
        520,
        0.95,
        0.25,
        30000
    ),
    event!(
        "444bf",
        "4x4x4 Blindfolded",
        "3",
        Some(120000),
        true,
        None,
        50000,
        60000,
        5100,
        6200,
        0.1,
        0.04,
        4000
    ),
    event!(
        "555bf",
        "5x5x5 Blindfolded",
        "3",
        Some(180000),
        true,
        None,
        110000,
        130000,
        12000,
        14000,
        0.05,
        0.03,
        2000
    ),
    event!(
        "333mbf",
        "3x3x3 Multi-Blind",
        "1",
        None,
        false,
        None,
        0,
        0,
        0,
        0,
        0.0,
        0.08,
        8000
    ),
];

pub fn find(id: &str) -> Option<&'static EventInfo> {
    EVENTS.iter().find(|event| event.id == id)
}

/// Encodes a multi-blind result the WCA way: `0DDTTTTTMM`.
pub fn encode_multi_blind(solved: u32, attempted: u32, seconds: u32) -> i64 {
    let missed = attempted - solved;
    let points = solved as i64 - missed as i64;
    (99 - points) * 10_000_000 + seconds as i64 * 100 + missed as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_multi_blind_like_the_wca() {
        // 9/10 (8 points) in 58:30, 1 missed.
        assert_eq!(encode_multi_blind(9, 10, 3510), 910351001);
    }

    #[test]
    fn cutoffs_are_below_time_limits() {
        for event in EVENTS.iter() {
            if let (Some((_, cutoff)), Some(limit)) = (event.cutoff, event.time_limit) {
                assert!(cutoff < limit, "{}", event.id);
            }
        }
    }
}
