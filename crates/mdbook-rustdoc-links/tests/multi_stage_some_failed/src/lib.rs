#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
pub fn amd64_only() {}

#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
pub fn arm64_only() {
    42
}
