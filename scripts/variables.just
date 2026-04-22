cargo_profile := env("CARGO_PROFILE", "release")
cargo_target_dir := if cargo_profile == "dev" { "debug" } else { cargo_profile }
