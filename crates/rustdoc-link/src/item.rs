use syn::{
    parse::{Parse, ParseStream},
    spanned::Spanned,
    Error, Expr, ExprCall, ExprMacro, ExprPath, Ident, Macro, Path, Result,
};
use tap::Pipe;

pub enum ItemName {
    Path { path: Path },
    Call { path: Path },
    Macro { mac: Macro },
}

impl Parse for ItemName {
    fn parse(input: ParseStream) -> Result<Self> {
        let expr = Expr::parse(input)?;
        match expr {
            Expr::Path(ExprPath {
                path, qself: None, ..
            }) => Ok(Self::Path { path }),

            Expr::Call(ExprCall { func, .. }) => match *func {
                Expr::Path(ExprPath {
                    path, qself: None, ..
                }) => Ok(Self::Call { path }),
                func => Error::new(func.span(), "expected a path").pipe(Err),
            },

            Expr::Macro(ExprMacro { mac, .. }) => Ok(Self::Macro { mac }),

            expr => Error::new(expr.span(), "expected a path, call, or macro").pipe(Err),
        }
    }
}

impl ItemName {
    pub fn ident(&self) -> &Ident {
        let path = match &self {
            Self::Path { path } => path,
            Self::Call { path, .. } => path,
            Self::Macro { mac } => &mac.path,
        };
        &path
            .segments
            .last()
            .expect("path should not be empty")
            .ident
    }
}

#[cfg(test)]
const _: () = {
    use proc_macro2::TokenStream;
    use quote::{quote, ToTokens};

    impl ToTokens for ItemName {
        fn to_tokens(&self, tokens: &mut TokenStream) {
            match self {
                Self::Path { path } => path.to_tokens(tokens),
                Self::Call { path, .. } => quote! { #path () }.to_tokens(tokens),
                Self::Macro { mac } => mac.to_tokens(tokens),
            }
        }
    }
};
