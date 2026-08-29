# Validate

A sign-up form validated with `#[derive(Schema)]`, showing both ways to handle the result:

- `/extractor`: the `Valid<SignUp>` extractor in the handler signature rejects invalid submissions with a `400 Bad Request` before the handler runs.
- `/manual`: the handler pattern-matches on the outcome of the `Validation<SignUp>` extractor, redirecting to a welcome page on success and re-rendering the form with the errors next to their fields on failure.

Run it with:

```sh
cargo topcoat dev -p validate
```
