//! Invented people and places. Surnames are built from syllables so the
//! generated people don't copy anyone real.

use super::rng::Rng;

const FEMALE_FIRST_NAMES: [&str; 24] = [
    "Ada", "Alma", "Bea", "Cora", "Daria", "Elin", "Fiona", "Greta", "Hana", "Ida", "Julia",
    "Kira", "Lena", "Maja", "Nina", "Olga", "Pia", "Rita", "Sara", "Tara", "Una", "Vera", "Wanda",
    "Zoe",
];

const MALE_FIRST_NAMES: [&str; 24] = [
    "Adam", "Bruno", "Cyril", "Dawid", "Emil", "Felix", "Gustav", "Hugo", "Igor", "Jonas", "Karol",
    "Leon", "Marek", "Nils", "Oskar", "Pavel", "Rafal", "Simon", "Tomas", "Viktor", "Wiktor",
    "Xavier", "Yann", "Zeno",
];

const OTHER_FIRST_NAMES: [&str; 8] = [
    "Alex", "Charlie", "Robin", "Sam", "Kim", "Noa", "Ari", "Jule",
];

const SURNAME_STARTS: [&str; 24] = [
    "Bral", "Dorv", "Falk", "Grem", "Hask", "Jorv", "Kelm", "Lund", "Marv", "Norb", "Ostr", "Prev",
    "Quel", "Rask", "Selv", "Torm", "Ulvr", "Varn", "Welt", "Yarn", "Zelm", "Kovr", "Brin", "Tavr",
];

const SURNAME_MIDDLES: [&str; 8] = ["a", "e", "i", "o", "an", "er", "il", "un"];

const SURNAME_ENDS: [&str; 14] = [
    "ski", "wick", "sen", "ova", "ez", "ari", "dor", "lin", "mont", "vik", "ard", "ell", "ton",
    "ko",
];

pub struct Country {
    pub iso2: &'static str,
    pub name: &'static str,
    pub continent_id: &'static str,
}

pub const COUNTRIES: [Country; 14] = [
    Country {
        iso2: "PL",
        name: "Poland",
        continent_id: "_Europe",
    },
    Country {
        iso2: "DE",
        name: "Germany",
        continent_id: "_Europe",
    },
    Country {
        iso2: "CZ",
        name: "Czech Republic",
        continent_id: "_Europe",
    },
    Country {
        iso2: "GB",
        name: "United Kingdom",
        continent_id: "_Europe",
    },
    Country {
        iso2: "FR",
        name: "France",
        continent_id: "_Europe",
    },
    Country {
        iso2: "ES",
        name: "Spain",
        continent_id: "_Europe",
    },
    Country {
        iso2: "UA",
        name: "Ukraine",
        continent_id: "_Europe",
    },
    Country {
        iso2: "LT",
        name: "Lithuania",
        continent_id: "_Europe",
    },
    Country {
        iso2: "US",
        name: "United States",
        continent_id: "_North America",
    },
    Country {
        iso2: "CA",
        name: "Canada",
        continent_id: "_North America",
    },
    Country {
        iso2: "BR",
        name: "Brazil",
        continent_id: "_South America",
    },
    Country {
        iso2: "AU",
        name: "Australia",
        continent_id: "_Oceania",
    },
    Country {
        iso2: "JP",
        name: "Japan",
        continent_id: "_Asia",
    },
    Country {
        iso2: "CN",
        name: "China",
        continent_id: "_Asia",
    },
];

pub fn country(iso2: &str) -> &'static Country {
    COUNTRIES
        .iter()
        .find(|country| country.iso2 == iso2)
        .expect("every generated country is in COUNTRIES")
}

/// What the venue timezone implies about the host country.
pub struct Host {
    pub timezone: &'static str,
    pub country_iso2: &'static str,
    pub currency_code: &'static str,
    pub base_entry_fee: u32,
    /// Somewhere inside the country, not a real venue.
    pub latitude_microdegrees: i64,
    pub longitude_microdegrees: i64,
}

pub const HOSTS: [Host; 13] = [
    Host {
        timezone: "Europe/Warsaw",
        country_iso2: "PL",
        currency_code: "PLN",
        base_entry_fee: 5000,
        latitude_microdegrees: 52_000_000,
        longitude_microdegrees: 19_000_000,
    },
    Host {
        timezone: "Europe/Berlin",
        country_iso2: "DE",
        currency_code: "EUR",
        base_entry_fee: 1500,
        latitude_microdegrees: 51_000_000,
        longitude_microdegrees: 10_000_000,
    },
    Host {
        timezone: "Europe/Prague",
        country_iso2: "CZ",
        currency_code: "CZK",
        base_entry_fee: 30000,
        latitude_microdegrees: 49_800_000,
        longitude_microdegrees: 15_500_000,
    },
    Host {
        timezone: "Europe/London",
        country_iso2: "GB",
        currency_code: "GBP",
        base_entry_fee: 1500,
        latitude_microdegrees: 52_500_000,
        longitude_microdegrees: -1_500_000,
    },
    Host {
        timezone: "Europe/Paris",
        country_iso2: "FR",
        currency_code: "EUR",
        base_entry_fee: 1500,
        latitude_microdegrees: 46_500_000,
        longitude_microdegrees: 2_500_000,
    },
    Host {
        timezone: "Europe/Madrid",
        country_iso2: "ES",
        currency_code: "EUR",
        base_entry_fee: 1500,
        latitude_microdegrees: 40_000_000,
        longitude_microdegrees: -3_700_000,
    },
    Host {
        timezone: "America/New_York",
        country_iso2: "US",
        currency_code: "USD",
        base_entry_fee: 3000,
        latitude_microdegrees: 40_000_000,
        longitude_microdegrees: -76_000_000,
    },
    Host {
        timezone: "America/Chicago",
        country_iso2: "US",
        currency_code: "USD",
        base_entry_fee: 3000,
        latitude_microdegrees: 41_000_000,
        longitude_microdegrees: -89_000_000,
    },
    Host {
        timezone: "America/Los_Angeles",
        country_iso2: "US",
        currency_code: "USD",
        base_entry_fee: 3000,
        latitude_microdegrees: 36_000_000,
        longitude_microdegrees: -119_000_000,
    },
    Host {
        timezone: "America/Toronto",
        country_iso2: "CA",
        currency_code: "CAD",
        base_entry_fee: 3500,
        latitude_microdegrees: 44_000_000,
        longitude_microdegrees: -79_000_000,
    },
    Host {
        timezone: "America/Sao_Paulo",
        country_iso2: "BR",
        currency_code: "BRL",
        base_entry_fee: 8000,
        latitude_microdegrees: -23_000_000,
        longitude_microdegrees: -47_000_000,
    },
    Host {
        timezone: "Australia/Sydney",
        country_iso2: "AU",
        currency_code: "AUD",
        base_entry_fee: 4000,
        latitude_microdegrees: -33_500_000,
        longitude_microdegrees: 150_500_000,
    },
    Host {
        timezone: "Asia/Tokyo",
        country_iso2: "JP",
        currency_code: "JPY",
        base_entry_fee: 3000,
        latitude_microdegrees: 35_500_000,
        longitude_microdegrees: 139_500_000,
    },
];

pub fn host(timezone: &str) -> Option<&'static Host> {
    HOSTS.iter().find(|host| host.timezone == timezone)
}

pub fn first_name(rng: &mut Rng, gender: &str) -> &'static str {
    match gender {
        "f" => rng.pick(&FEMALE_FIRST_NAMES),
        "m" => rng.pick(&MALE_FIRST_NAMES),
        _ => rng.pick(&OTHER_FIRST_NAMES),
    }
}

pub fn surname(rng: &mut Rng) -> String {
    format!(
        "{}{}{}",
        rng.pick(&SURNAME_STARTS),
        rng.pick(&SURNAME_MIDDLES),
        rng.pick(&SURNAME_ENDS)
    )
}

/// The host country most of the time, otherwise a random other one.
pub fn country_iso2(rng: &mut Rng, host_iso2: &'static str) -> &'static str {
    if rng.chance(0.75) {
        host_iso2
    } else {
        rng.pick(&COUNTRIES).iso2
    }
}

/// A fixed, invented account FKMTime tests can sign in with.
pub struct Account {
    pub key: &'static str,
    pub name: &'static str,
    pub wca_id: &'static str,
    pub user_id: u64,
    pub gender: &'static str,
    pub delegate_status: Option<&'static str>,
    /// WCA team, e.g. `wst`, which gives access to every competition's WCIF.
    pub team: Option<&'static str>,
    /// Role in the generated competition, if any.
    pub role: Option<&'static str>,
    pub description: &'static str,
}

pub const ACCOUNTS: [Account; 4] = [
    Account {
        key: "delegate",
        name: "Delwyn Vaqrell",
        wca_id: "2012VAQR01",
        user_id: 9_000_001,
        gender: "m",
        delegate_status: Some("delegate"),
        team: None,
        role: Some("delegate"),
        description: "Delegate of the generated competition",
    },
    Account {
        key: "organizer",
        name: "Orla Yqvist",
        wca_id: "2015YQVI01",
        user_id: 9_000_002,
        gender: "f",
        delegate_status: None,
        team: None,
        role: Some("organizer"),
        description: "Organizer (not a delegate) of the generated competition",
    },
    Account {
        key: "outsider",
        name: "Otis Kexwold",
        wca_id: "2017KEXW01",
        user_id: 9_000_003,
        gender: "m",
        delegate_status: None,
        team: None,
        role: None,
        description: "No rights to the generated competition",
    },
    Account {
        key: "admin",
        name: "Ada Ulqarsen",
        wca_id: "2011ULQA01",
        user_id: 9_000_004,
        gender: "f",
        delegate_status: None,
        team: Some("wst"),
        role: None,
        description: "WST member, can read every WCIF",
    },
];

pub fn account(wca_id: &str) -> Option<&'static Account> {
    ACCOUNTS
        .iter()
        .find(|account| account.wca_id.eq_ignore_ascii_case(wca_id))
}
