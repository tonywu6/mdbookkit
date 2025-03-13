use anyhow::Context;
use syn::{
    parenthesized,
    parse::{End, Parse, ParseStream, Parser},
    spanned::Spanned,
    token::Paren,
    PathArguments, QSelf, Token, TypePath,
};
use tap::TapFallible;

use crate::log_debug;

#[derive(Debug)]
pub struct Item {
    pub stmt: String,
    pub cols: Carets,
    pub key: String,
    pub fragment: Option<String>,
}

impl Item {
    pub fn parse(input: &str) -> anyhow::Result<Self> {
        let (path, fragment) = match input.split_once('#') {
            None => (input, None),
            Some((path, fragment)) => (path, Some(fragment)),
        };

        let item = ItemName::parse.parse_str(path)?;

        let (name, column) = {
            let mut name = String::new();
            let mut column = 0;

            let gt = if let Some(QSelf { ty, position, .. }) = item.path.qself {
                name.push('<');
                name.push_str(&input[ty.span().byte_range()]);
                name.push_str(" as ");
                Some(position - 1)
            } else {
                None
            };

            for (idx, chunk) in item.path.path.segments.pairs().enumerate() {
                column = name.len();

                let leading = &chunk.value().ident.span();

                name.push_str(&input[leading.span().byte_range()]);

                match &chunk.value().arguments {
                    PathArguments::None => {}

                    PathArguments::AngleBracketed(args) => {
                        if args.colon2_token.is_none() {
                            // make it a turbofish
                            name.push_str("::");
                        }
                        name.push_str(&input[args.span().byte_range()])
                    }

                    PathArguments::Parenthesized(args) => {
                        name.push_str(&input[args.span().byte_range()])
                    }
                }

                if gt == Some(idx) {
                    name.push('>');
                }

                if let Some(punct) = chunk.punct() {
                    name.push_str(&input[punct.span().byte_range()]);
                }
            }

            (name, column)
        };

        let (stmt, cols) = match item.kind {
            None => {
                let pattern = "let _: ";
                let assign = " = ";
                let stmt = format!("{pattern}{name}{assign}{name};");

                let c1 = pattern.len() + column;
                let c2 = pattern.len() + name.len() + assign.len() + column;
                let cols = Carets::Decl(c1, c2);

                (stmt, cols)
            }
            Some(ItemKind::Call) => (format!("{name}();"), Carets::Expr(column)),
            Some(ItemKind::Macro) => (format!("{name}!();"), Carets::Expr(column)),
        };

        let key = input.into();
        let fragment = fragment.map(Into::into);

        Ok(Self {
            stmt,
            cols,
            key,
            fragment,
        })
    }

    pub fn parse_all<R, T>(iter: R) -> Vec<Self>
    where
        R: Iterator<Item = T>,
        T: AsRef<str>,
    {
        iter.filter_map(|link| {
            Item::parse(link.as_ref())
                .with_context(|| format!("could not parse {:?}", link.as_ref()))
                .tap_err(log_debug!())
                .ok()
        })
        .collect()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Carets {
    Decl(usize, usize),
    Expr(usize),
}

struct ItemName {
    path: TypePath,
    kind: Option<ItemKind>,
}

#[derive(Debug, Clone, Copy)]
enum ItemKind {
    Call,
    Macro,
}

impl Parse for ItemName {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let path = input.parse::<TypePath>()?;
        if input.peek(Token![!]) {
            // as a macro
            input.parse::<Token![!]>()?;
            input.step(|cursor| {
                if let Some((inner, _, _, rest)) = cursor.any_group() {
                    if inner.eof() {
                        Ok(((), rest))
                    } else {
                        Err(cursor.error("expected no arguments"))
                    }
                } else {
                    Ok(((), *cursor))
                }
            })?;
            input.parse::<Eof>()?;
            Ok(Self {
                path,
                kind: Some(ItemKind::Macro),
            })
        } else if input.peek(Paren) {
            // as a function
            let inner;
            let _ = parenthesized!(inner in input);
            if inner.is_empty() {
                Ok(Self {
                    path,
                    kind: Some(ItemKind::Call),
                })
            } else {
                Err(input.error("expected no arguments"))
            }
        } else {
            // as a path
            input.parse::<Eof>()?;
            Ok(Self { path, kind: None })
        }
    }
}

struct Eof;

impl Parse for Eof {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if input.peek(End) {
            Ok(Self)
        } else {
            Err(input.error("expected no additional token"))
        }
    }
}
