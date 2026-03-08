use anyhow::{Context, Result};
use syn::{
    PathArguments, QSelf, Token, TypePath, parenthesized,
    parse::{End, Parse, ParseStream, Parser},
    spanned::Spanned,
    token::Paren,
};
use tracing::trace;

use mdbookkit::write_str;

use crate::markup::AttributedString;

/// Text that look like Rust items.
#[derive(Debug)]
pub struct Item {
    /// The parsed item name, which may be different from the source text (e.g.
    /// turbofish are expanded).
    pub qualified: String,
    /// Syntactically valid statement that rust-analyzer can understand.
    pub statement: AttributedString<()>,
}

impl Item {
    pub fn new(source: &str) -> Result<Self> {
        let syntax = ItemName::parse
            .parse_str(source)
            .context("could not parse as an item name")?;

        let qualified = {
            let mut qualified = AttributedString::new();

            let gt = if let Some(QSelf { ty, position, .. }) = syntax.path.qself {
                trace!("fully qualified syntax");
                write_str!(qualified, "<");
                write_str!(qualified, "{}", &source[ty.span().byte_range()]);
                write_str!(qualified, " as ");
                Some(position - 1)
            } else {
                None
            };

            for (idx, chunk) in syntax.path.path.segments.pairs().enumerate() {
                if idx == syntax.path.path.segments.pairs().len() - 1 {
                    qualified.markup(());
                }

                let leading = &chunk.value().ident.span();

                write_str!(qualified, "{}", &source[leading.span().byte_range()]);

                match &chunk.value().arguments {
                    PathArguments::None => {}

                    PathArguments::AngleBracketed(args) => {
                        if args.colon2_token.is_none() {
                            trace!("turbofish");
                            // make it a turbofish
                            write_str!(qualified, "::");
                        }
                        write_str!(qualified, "{}", &source[args.span().byte_range()]);
                    }

                    PathArguments::Parenthesized(args) => {
                        write_str!(qualified, "{}", &source[args.span().byte_range()]);
                    }
                }

                if gt == Some(idx) {
                    write_str!(qualified, ">");
                }

                if let Some(punct) = chunk.punct() {
                    write_str!(qualified, "{}", &source[punct.span().byte_range()]);
                }
            }

            qualified
        };

        trace!(qualified = ?qualified.text(), kind = ?syntax.kind);

        let statement = match syntax.kind {
            None => {
                let mut statement = AttributedString::new();
                write_str!(statement, "let _: ");
                statement.append(qualified.clone());
                write_str!(statement, " = ");
                statement.append(qualified.clone());
                write_str!(statement, ";");
                statement
            }
            Some(ItemKind::Call) => {
                let mut statement = qualified.clone();
                write_str!(statement, "();");
                statement
            }
            Some(ItemKind::Macro) => {
                let mut statement = qualified.clone();
                write_str!(statement, "!();");
                statement
            }
        };

        Ok(Self {
            qualified: qualified.text().to_owned(),
            statement,
        })
    }
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
