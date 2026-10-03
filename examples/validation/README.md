# Validation example

Demonstrates the `validation` feature: manual `Validate` impls, nested struct validation, HTML form re-rendering, and JSON API responses.

## Run

```sh
cargo run -p validation
```

The server starts on `http://127.0.0.1:3000` by default.

## Try it

GET `/signup` returns the empty HTML form:

```sh
curl http://localhost:3000/signup
```

Submit the form with an empty name or an invalid email to see inline errors:

```sh
curl -X POST http://localhost:3000/signup \
  -H "Content-Type: application/x-www-form-urlencoded" \
  -d 'name=&email=bad'
```

Test the JSON API with a nested address to see nested validation:

```sh
curl -X POST http://localhost:3000/api/users \
  -H "Content-Type: application/json" \
  -d '{"name":"Ada","email":"not-an-email","address":{"city":"","zip":"12345"}}'
```

The response is `422 Unprocessable Entity` with RFC 9457 problem details (`application/problem+json`).
