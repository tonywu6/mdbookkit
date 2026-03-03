use anyhow::{Context, Result};
use syn::{
    PathArguments, QSelf, Token, TypePath, parenthesized,
    parse::{End, Parse, ParseStream, Parser},
    spanned::Spanned,
    token::Paren,
};
use tracing::trace;

/// Text that look like Rust items.
#[derive(Debug)]
pub struct Item {
    /// The parsed item name, which may be different from the source text (e.g.
    /// turbofish are expanded).
    pub qualified: String,
    /// Syntactically valid statement that rust-analyzer can understand.
    pub statement: String,
    /// "Points of interest" in [`stmt`][Self::stmt] that can be used to construct.
    /// [`TextDocumentPositionParams`][lsp_types::TextDocumentPositionParams].
    pub cursors: Cursors,
}

impl Item {
    pub fn new(source: &str) -> Result<Self> {
        let syntax = ItemName::parse
            .parse_str(source)
            .context("could not parse as an item name")?;

        let (qualified, column) = {
            let mut qualified = String::new();
            let mut column = 0;

            let gt = if let Some(QSelf { ty, position, .. }) = syntax.path.qself {
                trace!("fully qualified syntax");
                qualified.push('<');
                qualified.push_str(&source[ty.span().byte_range()]);
                qualified.push_str(" as ");
                Some(position - 1)
            } else {
                None
            };

            for (idx, chunk) in syntax.path.path.segments.pairs().enumerate() {
                column = qualified.len();

                let leading = &chunk.value().ident.span();

                qualified.push_str(&source[leading.span().byte_range()]);

                match &chunk.value().arguments {
                    PathArguments::None => {}

                    PathArguments::AngleBracketed(args) => {
                        if args.colon2_token.is_none() {
                            trace!("turbofish");
                            // make it a turbofish
                            qualified.push_str("::");
                        }
                        qualified.push_str(&source[args.span().byte_range()])
                    }

                    PathArguments::Parenthesized(args) => {
                        qualified.push_str(&source[args.span().byte_range()])
                    }
                }

                if gt == Some(idx) {
                    qualified.push('>');
                }

                if let Some(punct) = chunk.punct() {
                    qualified.push_str(&source[punct.span().byte_range()]);
                }
            }

            (qualified, column)
        };

        trace!(?qualified, kind = ?syntax.kind);

        let (statement, cursors) = match syntax.kind {
            None => {
                let pattern = "let _: ";
                let assign = " = ";
                let text = format!("{pattern}{qualified}{assign}{qualified};");

                let c1 = pattern.len() + column;
                let c2 = pattern.len() + qualified.len() + assign.len() + column;
                let cursors = Cursors::Decl([c1, c2]);

                (text, cursors)
            }
            Some(ItemKind::Call) => (format!("{qualified}();"), Cursors::Expr([column])),
            Some(ItemKind::Macro) => (format!("{qualified}!();"), Cursors::Expr([column])),
        };

        Ok(Self {
            qualified,
            statement,
            cursors,
        })
    }
}

#[derive(Debug)]
pub enum Cursors {
    Decl([usize; 2]),
    Expr([usize; 1]),
}

impl AsRef<[usize]> for Cursors {
    fn as_ref(&self) -> &[usize] {
        match self {
            Self::Decl(c) => c,
            Self::Expr(c) => c,
        }
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
