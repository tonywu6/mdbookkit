use mdbook_markdown::pulldown_cmark::{
    BrokenLink, BrokenLinkCallback, CowStr, Event, Options, Parser,
};
use tap::Pipe;

use mdbookkit::markdown::default_markdown_options;

pub fn stream(text: &str, options: Options) -> MarkdownStream<'_> {
    Parser::new_with_broken_link_callback(text, options, Some(ItemLinks))
}

pub type MarkdownStream<'a> = Parser<'a, ItemLinks>;

/// [`BrokenLinkCallback`] implementation that unconditionally converts all "broken"
/// links to links to be further processed.
///
/// "Broken" links are links like `[text][link::item]` that don't have associated URLs.
/// Such links are expected for this preprocessor.
///
/// Links that are "broken" that aren't actually doc links won't show up in the output,
/// because the preprocessor ignores links that cannot be parsed and is capable of
/// emitting only changed links, see [`PatchStream`][mdbookkit::markdown::PatchStream].
pub struct ItemLinks;

impl ItemLinks {
    // Explicitly disable smart punctuation to prevent quotes from being changed
    // or else things like lifetimes may become invalid
    const OPTIONS: Options =
        default_markdown_options().intersection(Options::ENABLE_SMART_PUNCTUATION.complement());
}

impl<'input> BrokenLinkCallback<'input> for ItemLinks {
    fn handle_broken_link(
        &mut self,
        link: BrokenLink<'input>,
    ) -> Option<(CowStr<'input>, CowStr<'input>)> {
        // try to strip away inline markups in order to support stylized shorthand links
        // for example, this extracts "std" from [`std`], removing the `inline code` markup
        let inner = if let CowStr::Borrowed(inner) = link.reference {
            // this is currently done by manually parsing the inner text, filtering
            // the event stream, and then re-emitting it as text
            //
            // because of the 'input lifetime, this can only be done on CowStr::Borrowed,
            // otherwise the re-emitted text "may not live long enough."
            //
            // this should be okay in usage, because this is only called by the Parser,
            // which should only provide borrowed text.

            let parse = stream(inner, Self::OPTIONS);

            let inner = parse
                .filter_map(|event| match event {
                    Event::Text(inner) => Some(inner),
                    Event::Code(inner) => Some(inner),
                    _ => None,
                })
                .collect::<Vec<_>>();

            if inner.len() == 1 {
                inner.into_iter().next().expect("has 1 item")
            } else {
                inner
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Box<str>>()
                    .pipe(CowStr::Boxed)
            }
        } else {
            link.reference.clone()
        };
        if inner.is_empty() {
            None
        } else {
            let title = inner.clone();
            Some((inner, title))
        }
    }
}
