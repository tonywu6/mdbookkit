# 1st-party

- [`Earth`](https://docs.rs/rustdoc/0.1.0/rustdoc/struct.Earth.html "struct rustdoc::Earth")
- [`Earth::type`](https://docs.rs/rustdoc/0.1.0/rustdoc/struct.Earth.html#structfield.type "field rustdoc::Earth::type") (no need to prefix `r#`)
- [`Earth::moon`](https://docs.rs/rustdoc/0.1.0/rustdoc/struct.Earth.html#structfield.moon "field rustdoc::Earth::moon")

Preludes are automatically injected if workspace has only one crate, in which case
`crate::...` and `self::...` will work.

- [`crate::Earth`](https://docs.rs/rustdoc/0.1.0/rustdoc/struct.Earth.html "struct rustdoc::Earth")
- [`self::Earth::artemis`](https://docs.rs/rustdoc/0.1.0/rustdoc/struct.Earth.html#structfield.artemis "field rustdoc::Earth::artemis")

# 3rd-party

- [`tap`](https://docs.rs/tap/1.0.1/tap/index.html "mod tap")
- [`tap::Tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html "trait tap::tap::Tap")
- [`tap::Tap::tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html#method.tap "method tap::tap::Tap::tap")
- [`::tap::Tap::tap`](https://docs.rs/tap/1.0.1/tap/tap/trait.Tap.html#method.tap "method tap::tap::Tap::tap")
