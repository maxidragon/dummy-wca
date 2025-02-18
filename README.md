# dummy-wca

This is a dummy WCA server that can be used for testing purposes. It is a Axum (Rust) app with a few endpoints.

This project was mainly created for testing the [FKMTime](https://github.com/FKMTime/FKMTime) project.

## Endpoints

### `GET /api/v0/competitions?managed_by_me=true`

Returns a list of manageable competitions. Requires the `Authorization` header to be set to `Bearer <user-token>`. Every user has their own token.

### `GET /api/v0/competitions/:id`

Returns the information of a competition.

### `GET /api/v0/competitions/:id/wcif`

Returns the WCIF of a competition.

Requires the `Authorization` header to be set to `Bearer <token>`.

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

Returns a list of persons that match the query.

### `GET /api/v0/users/:id`

Returns the user with the given WCA ID.

## Running

### Directly with cargo

Ensure you have Cargo installed. Then run the following command:

```bash
cargo run
```

## With docker

Ensure you have Docker installed. Then run the following command:

```bash
docker run -p 3000:3000 maxidragon/dummy-wca
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