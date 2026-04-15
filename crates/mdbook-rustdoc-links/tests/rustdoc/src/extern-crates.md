# 1st-party

- [`Earth`]
- [`Earth::type`] (no need to prefix `r#`)
- [`Earth::moon`]

Preludes are automatically injected if workspace has only one crate, in which case
`crate::...` and `self::...` will work.

- [`crate::Earth`]
- [`self::Earth::artemis`]

# 3rd-party

- [`tap`]
- [`tap::Tap`]
- [`tap::Tap::tap`]
- [`::tap::Tap::tap`]
