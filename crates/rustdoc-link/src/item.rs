use syn::{
    parenthesized,
    parse::{End, Parse, ParseStream, Parser},
    spanned::Spanned,
    token::Paren,
    PathArguments, QSelf, Token, TypePath,
};

#[derive(Debug)]
pub struct Item {
    pub name: String,
    pub stmt: String,
    pub cursor: Cursor,
}

impl Item {
    pub fn parse(path: &str) -> anyhow::Result<Self> {
        let path = match path.split_once('@') {
            None => path,
            Some((_, path)) => path,
        };

        let item = ItemName::parse.parse_str(path)?;

        let (name, column) = {
            let mut name = String::new();
            let mut column = 0;

            let gt = if let Some(QSelf { ty, position, .. }) = item.path.qself {
                name.push('<');
                name.push_str(&path[ty.span().byte_range()]);
                name.push_str(" as ");
                Some(position - 1)
            } else {
                None
            };

            for (idx, chunk) in item.path.path.segments.pairs().enumerate() {
                column = name.len();

                let leading = &chunk.value().ident.span();

                name.push_str(&path[leading.span().byte_range()]);

                match &chunk.value().arguments {
                    PathArguments::None => {}

                    PathArguments::AngleBracketed(args) => {
                        if args.colon2_token.is_none() {
                            // make it a turbofish
                            name.push_str("::");
                        }
                        name.push_str(&path[args.span().byte_range()])
                    }

                    PathArguments::Parenthesized(args) => {
                        name.push_str(&path[args.span().byte_range()])
                    }
                }

                if gt == Some(idx) {
                    name.push('>');
                }

                if let Some(punct) = chunk.punct() {
                    name.push_str(&path[punct.span().byte_range()]);
                }
            }

            (name, column)
        };

        let (stmt, cursor) = match item.kind {
            None => {
                let pattern = "let _: ";
                let assign = " = ";
                let stmt = format!("{pattern}{name}{assign}{name};");

                let c1 = pattern.len() + column;
                let c2 = pattern.len() + name.len() + assign.len() + column;
                let cursor = Cursor::Decl([c1, c2]);

                (stmt, cursor)
            }
            Some(ItemKind::Call) => (format!("{name}();"), Cursor::Expr([column])),
            Some(ItemKind::Macro) => (format!("{name}!();"), Cursor::Expr([column])),
        };

        Ok(Self { name, stmt, cursor })
    }
}

#[derive(Debug)]
pub enum Cursor {
    Decl([usize; 2]),
    Expr([usize; 1]),
}

impl AsRef<[usize]> for Cursor {
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
