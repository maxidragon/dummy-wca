# dummy-wca

This is a dummy WCA server that can be used for testing purposes. It is a Axum (Rust) app with a few endpoints.

This project was mainly created for testing the [FKMTime](https://github.com/FKMTime/FKMTime) project.

## Endpoints

### `GET /api/v0/competitions?managed_by_me=true`

Returns a list of manageable competitions. Requires the `Authorization` header to be set to `Bearer example-access-token`.

### `GET /api/v0/competitions/:id/wcif`

Returns the WCIF of a competition.

Requires the `Authorization` header to be set to `Bearer example-access-token`.

### `GET /api/v0/competitions/:id/wcif/public`

Returns the public WCIF of a competition. Does not require any authentication.

### `GET /api/v0/me`

Returns the current user information. Requires the `Authorization` header to be set to `Bearer example-access-token`.

### `GET /oauth/authorize`

Redirects to the redirect URL with the `code` query parameter.

### `POST /oauth/token`

Returns an access token (`example-access-token`) that can be used to authenticate requests.


