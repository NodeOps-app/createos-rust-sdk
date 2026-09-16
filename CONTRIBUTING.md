# Contributing

Install stable Rust, then run:

```sh
make check
make test
```

Public APIs should follow the [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/):
use conventional Rust naming, implement common traits, document public items,
keep wire-format compatibility explicit with Serde attributes, and return typed
errors instead of panicking.

Use Conventional Commits with an imperative, lowercase subject of at most 50
characters. Accepted types are `feat`, `fix`, `docs`, `style`, `refactor`,
`test`, `chore`, and `perf`.

