extern crate proc_macro;
use proc_macro::TokenStream;

#[proc_macro]
pub fn make_shelter(_item: TokenStream) -> TokenStream {
    r#"
        #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
        pub fn shelter() -> u32 { 42 }
    "#
    .parse()
    .unwrap()
}
