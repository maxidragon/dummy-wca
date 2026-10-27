# dummy-wca

This is a dummy WCA server that can be used for testing purposes. It is a Axum (Rust) app with a few endpoints.

This project was mainly created for testing the [FKMTime](https://github.com/FKMTime/FKMTime) project.

Besides the snapshot competitions in `data/`, it serves a [generated test competition](#generated-test-competition): invented people, results and schedule, starting on the day the server starts by default.

## Competitions page

Open `http://localhost:3000/` to see every competition the server knows (generated and snapshot) and to generate a new one: pick a name, start date, number of competitors, rooms, days, timezone, seed, and rounds per event, then press **Generate**. The new competition is served straight away by every endpoint below. The delegate and organizer accounts manage it, and it shows up in their `managed_by_me` list.

Competitions made this way live in memory until the server restarts. Each one can be removed from the page. The one from the `GENERATED_*` settings stays and is still the one `GET /dummy/generated` returns. To get different settings, generate a new competition. Existing ones aren't edited.

The form posts `application/x-www-form-urlencoded` to `POST /dummy/competitions`, so a script can do the same:

```bash
curl -d 'name=Small Test Open&start_date=2026-11-14&competitors=20&event_333=2&event_pyram=1' localhost:3000/dummy/competitions
```

Fields not sent keep the defaults from [Settings](#settings). Events are `event_<id>=<rounds>`, and the id is the name plus the year, as below.

## Endpoints

### `GET /api/v0/competitions?managed_by_me=true`

Returns a list of manageable competitions. Requires the `Authorization` header to be set to `Bearer <user-token>`. Every user has their own token.

For the generated competition's accounts, `start` and `end` filter like on the WCA: `start` keeps competitions ending on or after it, `end` keeps those starting on or before it. Both take a date or an ISO timestamp. Snapshot accounts always get their full list.

### `GET /api/v0/competitions/:id`

Returns the information of a competition.

### `GET /api/v0/competitions/:id/wcif`

Returns the WCIF of a competition.

Requires the `Authorization` header to be set to `Bearer <token>`.

### `GET /api/v0/competitions/:id/wcif/version/2`

Returns the version 2 WCIF of a competition. For the generated competition, a request without a token gets the public WCIF, and a request with a token gets the private one (403 if the user can't manage the competition). Of the snapshots, only those with a `wcif_v2.json` have one.

### `PATCH /api/v0/competitions/:id/wcif`

Saves `persons` (merged by `registrantId`) in memory. Every later WCIF request for that competition includes them, until a restart or `POST /dummy/reset`. The body may contain `id`, `formatVersion` and `persons`. Unknown registrant or activity ids give 400.

Requires the `Authorization` header to be set to `Bearer <token>` of a user who can manage the competition.

### `GET /api/v0/competitions/:id/wcif/public`

Returns the public WCIF of a competition. Does not require any authentication.

### `GET /api/v0/me`

Returns the current user information. Requires the `Authorization` header to be set to `Bearer <user-token>`. Every user has their own token.

### `GET /oauth/authorize`

Redirects to the page that asks to choose the user.

### `POST /oauth/token`

Returns an access token that can be used to authenticate requests.

### `GET /api/v0/records`

Returns a list of records.

### `GET /api/v0/persons?q=query`

Returns a list of persons whose name or WCA ID contains the query, ignoring case.

### `GET /api/v0/persons/:wca_id`

Returns one person.

### `GET /api/v0/users/:id`

Returns the user with the given WCA ID.

### `GET /dummy/generated`

Not a WCA endpoint. Returns the generated competition's current id, dates and settings, and the accounts that can sign in to it (with their `code-…` and `token-…`).

### `POST /dummy/reset`

Not a WCA endpoint. Clears saved patches for all competitions, or for one with `?id=<competition id>`.

## Generated test competition

The generated competition is invented from scratch: names, WCA IDs (about 15% newcomers have none), countries, birthdates, personal bests, registrations, rounds with time limits, cutoffs and advancement, a schedule with groups across rooms, and competitor, judge, scrambler and runner assignments for round 1. It is generated in a few milliseconds and cached.

The same settings always give the same data. Only the dates and the year in the id and name follow the start date, so `Dummy Cubing Open` is `DummyCubingOpen2026` in 2026 and `DummyCubingOpen2027` in 2027.

### Settings

Set these environment variables when starting the server (e.g. in `docker-compose`). Invalid values stop the server at startup with an explanation.

| Variable | Default | Meaning |
|---|---|---|
| `GENERATED_NAME` | `Dummy Cubing Open` | Name without the year. The id is its letters and digits plus the year. |
| `GENERATED_START_DATE` | today in the venue timezone, at server start | `YYYY-MM-DD`. Without it, the date is taken once when the server starts; restart it to move the competition to the new today. |
| `GENERATED_DAYS` | `2` | 1 to 4. |
| `GENERATED_COMPETITORS` | `60` | 2 to 1000 registrations; about 90% are accepted. |
| `GENERATED_EVENTS` | `333:3,222:2,444:2,666:1,333bf:1,333oh:2,clock:1,minx:1,pyram:2,skewb:1,sq1:1,444bf:1` | Events with their number of rounds (1 to 4). Rounds are dropped when there are too few competitors for them under the WCA Regulations. |
| `GENERATED_ROOMS` | `2` | 1 to 4. |
| `GENERATED_TIMEZONE` | `Europe/Warsaw` | Venue timezone; it also sets the host country and currency. Schedules keep their local times across DST changes. |
| `GENERATED_SEED` | `1` | Change it to get different people. |
| `PORT` | `3000` | Port to listen on. |

The defaults are chosen so that FKMTime can run its tests without any overrides. They include an Ao5 round without a cutoff, Ao5 and Mo3 rounds with cutoffs, Bo5 and Bo3 rounds with cumulative time limits, an event with three rounds, several groups per round, two rooms running a round at the same time, all four assignment codes (later rounds included), pending and deleted registrations, newcomers, and judges from the host country and from abroad. `default_competition_covers_what_fkmtime_uses` in `src/generator/tests.rs` pins this list.

### "Today"

dummy-wca decides which day the competition starts, so a test setup should ask it instead of working it out itself:

1. `GET /dummy/generated` for the current `id` and dates.
2. `GET /api/v0/competitions/<id>/wcif/version/2` with `Authorization: Bearer token-2012VAQR01` (the delegate) for the private WCIF to seed from.

"Today" is the date in the venue timezone, not the server's, so a container running in UTC and a test machine in Poland agree. It is taken **once, when the server starts**, and doesn't change while it runs: a stack left up overnight keeps the competition a database was seeded from, and its id doesn't change on 1 January. Restart the container to move the competition to the new today (and reseed), or set `GENERATED_START_DATE` to pin it. `GET /dummy/generated` shows `startDatePinned` and `serverStartedAt`.

### Accounts

| Account | WCA ID | User id | Role |
|---|---|---|---|
| Delwyn Vaqrell | `2012VAQR01` | 9000001 | Delegate |
| Orla Yqvist | `2015YQVI01` | 9000002 | Organizer, not a delegate |
| Otis Kexwold | `2017KEXW01` | 9000003 | No rights to the competition |
| Ada Ulqarsen | `2011ULQA01` | 9000004 | WST member; can read every WCIF |

Sign in with `code-<WCA ID>` or the buttons on `/oauth/authorize`. Every generated person with a WCA ID is a user too (`/api/v0/me`, `/api/v0/users/:id`, `/api/v0/persons`).

### Patches are shared

dummy-wca has no sessions: everyone using one server, for example several FKMTime checkouts pointed at the same local container, shares its patches. One session's `PATCH .../wcif` shows up in every other session's WCIF, and `POST /dummy/reset` without an id clears everyone's patches. In tests:

- Reset only your competition: `POST /dummy/reset?id=<competition id>`.
- Prefer patching persons your test owns, and reset before (not only after) a test that patches.
- For full isolation, run your own container.

## Running

### Directly with cargo

Ensure you have Cargo installed. Then run the following command:

```bash
cargo run
```

Run the tests with `cargo test`.

## With docker

Ensure you have Docker installed. Then run the following command:

```bash
docker run -p 3000:3000 maxidragon/dummy-wca
```

With settings for the generated competition:

```bash
docker run -p 3000:3000 -e GENERATED_COMPETITORS=120 -e GENERATED_START_DATE=2026-11-14 maxidragon/dummy-wca
```

## Adding to docker-compose

If you have dev docker compose file you can add the following service:

```yaml
dummy_wca:
  container_name: dummy-wca
  restart: unless-stopped
  image: maxidragon/dummy-wca:latest
  ports:
    - "3000:3000"
```