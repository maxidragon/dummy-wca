# dummy-wca

This is a dummy WCA server that can be used for testing purposes. It is a Axum (Rust) app with a few endpoints.

This project was mainly created for testing the [FKMTime](https://github.com/FKMTime/FKMTime) project.

Besides the snapshot competitions in `data/`, it serves a [generated test competition](#generated-test-competition): invented people, results and schedule, starting on the day the server starts by default.

## Competitions page

Open `http://localhost:3000/` to see every competition the server knows (generated and snapshot) and to generate a new one: pick a name, start date, number of competitors, rooms, days, timezone, seed, scoretaking software, and rounds per event, then press **Generate**. The new competition is served straight away by every endpoint below. The delegate and organizer accounts manage it, and it shows up in their `managed_by_me` list.

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

Returns the information of a competition. Like on the WCA, it includes `scoretaking_software`: `external`, `wca_live` or `internal`. Generated competitions take it from `GENERATED_SCORETAKING_SOFTWARE` or the page's form.

### `GET /api/v0/competitions/:id/wcif`

Returns the WCIF of a competition.

Requires the `Authorization` header to be set to `Bearer <token>`.

### `GET /api/v0/competitions/:id/wcif/version/2`

Returns the version 2 WCIF of a competition. For the generated competition, a request without a token gets the public WCIF, and a request with a token gets the private one (403 if the user can't manage the competition). Of the snapshots, only those with a `wcif_v2.json` have one.

### `PATCH /api/v0/competitions/:id/wcif`

Saves `persons` (merged by `registrantId`) and replaces `schedule`, `events` and `extensions` in memory. Every later WCIF request for that competition includes them, until a restart or `POST /dummy/reset`. The body may also contain `id` and `formatVersion`, and assignments may point at groups in the same body's `schedule`. Anything else, or an unknown registrant or activity id, gives 400.

Requires the `Authorization` header to be set to `Bearer <token>` of a user who can manage the competition.

### `PUT /api/v0/competitions/wcif/check`

Answers as `PATCH` would for the competition in the body's `id`, and saves nothing.

Requires the `Authorization` header to be set to `Bearer <token>` of a user who can manage the competition.

### `GET /api/v0/competitions/:id/wcif/public`

Returns the public WCIF of a competition. Does not require any authentication.

### `GET /api/v0/me`

Returns the current user information. Requires the `Authorization` header to be set to `Bearer <user-token>`. Every user has their own token.

### `GET /oauth/authorize`

Redirects to the page that asks to choose the user. Choosing one sends the browser to `redirect_uri` with `code`, and with `state` if the request had one. The code carries the requested `scope` (`public` if none); an unknown scope gives 400, as on the WCA.

### `POST /oauth/token`

Takes a JSON or form-encoded body, and returns `access_token`, `refresh_token`, `expires_in`, `scope` and `created_at`, like the WCA's Doorkeeper.

- `grant_type=authorization_code` with `code`. A code from the authorize page grants the scopes that were requested; a hand-written `code-<WCA ID>` grants every scope.
- `grant_type=refresh_token` with `refresh_token` gives a new pair with the same scopes. Each refresh token works once; a second use gives 400 `invalid_grant`.

Tokens last `TOKEN_LIFETIME_SECONDS` (default 7200, the WCA's 2 hours); after that they get 401. Set it lower to test refreshing.

The fixed `token-<WCA ID>` tokens (see [Accounts](#accounts)) never expire and have every scope.

### `GET /api/v0/records`

Returns a list of records.

### `GET /api/v0/persons?q=query`

Returns a list of persons whose name or WCA ID contains the query, ignoring case.

### `GET /api/v0/persons/:wca_id`

Returns one person.

### `GET /api/v0/users/:id`

Returns the user with the given WCA ID.

### `GET /api/v0/users/search/:query`

Returns `{ "result": [...] }` with the users whose name or WCA ID contains the query, ignoring case.

### `/api/v1` endpoints

Like on the WCA, these need a signed-in user unless they say otherwise, answer errors as `{"error": ...}` (401 `Please log in` without a valid token), and serve the generated competitions only. Each controller (registrations, scoretakers, live results) takes `API_RATE_LIMIT_PER_MINUTE` requests per minute per client IP (default 60, as on the WCA; 0 turns it off); after that it answers 429 with an empty body until the minute is over. The WCA doesn't limit its own internal network; dummy-wca limits everyone, so a client can test its 429 handling.

### `PATCH /api/v1/registrations/:id`

Replaces a registration's events: `{"competing": {"event_ids": ["333", "pyram"]}}`. The id is the WCIF's `registration.wcaRegistrationId`, and later WCIFs show the new events, until a restart or `POST /dummy/reset`. Answers `{"status": "ok", "registration": {...}}`.

As on the WCA, the token needs the `manage_registrations` scope (else 403 `-2003`), and the user must manage the competition or own the registration (else 401 `-2003`). Competitors may edit until the competition starts (403 `-4001` after); managers may edit any time. Events must be held at the competition, and at most `GENERATED_EVENTS_PER_REGISTRATION_LIMIT` (managers too); otherwise 422 `-4004`. dummy-wca only changes `event_ids`: other registration fields give 400.

### `GET /api/v1/competitions/:id/scoretakers`

Lists the users who may enter results besides the managers, as `[{"user_id", "name"}]`. Any signed-in user may list them.

### `POST /api/v1/competitions/:id/scoretakers`

Adds the user in `{"user_id": ...}` (a no-op if they already are one) and answers `{"user_id", "name"}`. Needs a manager (403 `Organizer privileges required`). Scoretakers are kept until a restart or `POST /dummy/reset`.

### `DELETE /api/v1/competitions/:id/scoretakers/:user_id`

Removes a scoretaker and answers them, or `null` if they weren't one. Needs a manager.

### Live results: `/api/v1/competitions/:id/live/rounds`

dummy-wca emulates the WCA's integrated live results for the generated competitions. Rounds come from the WCIF (after patches): their format, cutoff and `participationRuleset` (who proceeds from the previous round). Results live in memory until a restart or `POST /dummy/reset`.

Every live endpoint but `GET .../rounds/:round_id` needs `scoretaking_software` to be `internal` (403 `Scoretaking software needs to be set to Internal`). Those that change something need a signed-in manager or scoretaker (403 `Score taking privileges required`). Unknown rounds, registrations and results give 404.

| Endpoint | What it does |
|---|---|
| `GET /rounds/:round_id` | Public. The WCIF round with `round_id`, `competitors`, `results` (with `attempts` as `{value, attempt_number}`, `best`, `average`, `global_pos`, `advancing`, `advancing_questionable`, ...), `completed_competitors` and `state_hash`, which changes whenever a result does. |
| `GET /rounds` | Public. `{"rounds": [...]}`, each with its `state` and counts. |
| `PUT /rounds/:round_id/open` | Opens a `ready` round: an empty result for everyone accepted for the event (first round) or everyone proceeding from the previous round, whose results get locked. 400 if `pending`, `blocked` or already open. |
| `PUT /rounds/:round_id/clear` | Removes every attempt in an open round. |
| `DELETE /rounds/:round_id/close` | Removes an open round's results, only if nobody has a complete result yet, and unlocks the previous round. |
| `DELETE /rounds/:round_id/:registration_id` | Quits a competitor without attempts. With `advancing_ids` (query `advancing_ids[]=` or JSON body) in a later round, they must match who should proceed instead, or 400 with `should_advance`; those then join the round. |
| `DELETE /rounds/:round_id/bulk_quit` | The same for `registration_ids`. |
| `GET /rounds/:round_id/next_if_quit?registration_id=` | Who would proceed in that competitor's place: nobody, one, or several when tied. |
| `GET /rounds/:round_id/addable_competitors` | Who may be added: anyone accepted (first round) or anyone from the previous round. |
| `PUT /rounds/:round_id/:registration_id` | Adds a competitor to the round if it is open. |
| `PUT /rounds/:round_id/:registration_id/clear` | Removes one competitor's attempts. |

A round is `pending` until every result of the previous round is complete (all attempts, or the cutoff missed), `blocked` if the previous rounds had too few competitors for it under regulation 9m, `ready`, `open` once it has results, and `locked` once the next round opens. Who proceeds (`advancing`) follows the next round's `resultCondition` and the 75% rule (in a final, it marks the podium), and stops changing once the round is locked. Linked rounds, records, podiums and live updates over WebSockets aren't emulated.

### `GET /dummy/generated`

Not a WCA endpoint. Returns the generated competition's current id, dates and settings, and the accounts that can sign in to it (with their `code-…` and `token-…`).

### `POST /dummy/reset`

Not a WCA endpoint. Clears saved patches, registration changes, scoretakers and live results for all competitions, or for one with `?id=<competition id>`.

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
| `GENERATED_SCORETAKING_SOFTWARE` | `internal` | `external`, `wca_live` or `internal`, as the WCA's `scoretaking_software`. The WCA's own default is `external`; this one defaults to `internal` so the generated competition can be scoretaken through the live results API. |
| `PORT` | `3000` | Port to listen on. |
| `TOKEN_LIFETIME_SECONDS` | `7200` | How long a token from `POST /oauth/token` lasts. |
| `API_RATE_LIMIT_PER_MINUTE` | `60` | Requests per minute per client to each `/api/v1` controller. 0 turns the limit off. |
| `GENERATED_EVENTS_PER_REGISTRATION_LIMIT` | none | The most events one registration may have, enforced by `PATCH /api/v1/registrations/:id`. |

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