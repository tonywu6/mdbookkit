use mdbook_markdown::pulldown_cmark::{
    BrokenLink, BrokenLinkCallback, CowStr, Event, LinkType, Options, Parser,
};
use tap::Pipe;

use mdbookkit::markdown::default_markdown_options;

pub fn markdown(source: &str) -> Parser<'_, KeepBrokenLinks> {
    Parser::new_with_broken_link_callback(source, default_markdown_options(), Some(KeepBrokenLinks))
}

/// [`BrokenLinkCallback`] implementation that unconditionally converts all "broken"
/// links to links to be further processed.
///
/// "Broken" links are links like `[text][link::item]` that don't have associated URLs.
/// Such links are expected for this preprocessor.
///
/// Links that are "broken" that aren't actually doc links won't show up in the output,
/// because the preprocessor ignores links that cannot be parsed and is capable of
/// emitting only changed links, see [`PatchStream`][mdbookkit::markdown::PatchStream].
pub struct KeepBrokenLinks;

impl KeepBrokenLinks {
    const OPTIONS: Options =
        default_markdown_options().intersection(Options::ENABLE_SMART_PUNCTUATION.complement());
}

impl<'a> BrokenLinkCallback<'a> for KeepBrokenLinks {
    fn handle_broken_link(&mut self, link: BrokenLink<'a>) -> Option<(CowStr<'a>, CowStr<'a>)> {
        // try to strip away inline markups in order to support stylized shorthand links
        // for example, this extracts "std" from [`std`], removing the `inline code` markup

        let dest = if matches!(link.link_type, LinkType::Collapsed | LinkType::Shortcut)
            && let CowStr::Borrowed(dest) = link.reference
        {
            // this is currently done by manually parsing the inner text, filtering
            // the event stream, and then re-emitting it as text
            //
            // because of the input lifetime, this can only be done on CowStr::Borrowed,
            // otherwise the re-emitted text "may not live long enough."
            //
            // this should be okay in usage, because this is only called by the Parser,
            // which should only provide borrowed text.

            let elements = Parser::new_ext(dest, Self::OPTIONS)
                .filter_map(|event| match event {
                    Event::Text(inner) => Some(inner),
                    Event::Code(inner) => Some(inner),
                    // names with generics like `Vec<T>` will have `<T>` identified as
                    // inline HTML if the text is not marked as `code`
                    Event::InlineHtml(inner) => Some(inner),
                    _ => None,
                })
                .collect::<Vec<_>>();

            if elements.len() == 1 {
                (elements.into_iter()).next().expect("has 1 item")
            } else {
                (elements.iter())
                    .fold(String::with_capacity(dest.len()), |mut out, elem| {
                        out.push_str(elem);
                        out
                    })
                    .into_boxed_str()
                    .pipe(CowStr::Boxed)
            }
        } else {
            link.reference.clone()
        };

        if dest.is_empty() {
            None
        } else {
            let title = dest.clone();
            Some((dest, title))
        }
    }
}
