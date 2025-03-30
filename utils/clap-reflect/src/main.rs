use std::io::{Read, Write};

use anyhow::Result;
use clap::{ArgAction, CommandFactory, Parser, Subcommand, ValueEnum};
use mdbook::{book::Book, preprocess::PreprocessorContext, BookItem};
use minijinja::render;
use serde::Serialize;

use tap::Pipe;

#[derive(Parser, Debug, Clone)]
struct Program {
    #[arg(long, value_enum)]
    reflect: OptionType,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand, Debug, Clone)]
enum Command {
    Supports { renderer: String },
    Describe { ty: OptionType },
}

#[derive(ValueEnum, Debug, Clone)]
#[clap(rename_all = "kebab-case")]
enum OptionType {
    RustdocLinkOptions,
    LinkForeverOptions,
}

impl OptionType {
    fn tag(&self) -> String {
        let tag = self.to_possible_value().unwrap().get_name().to_owned();
        format!("<{tag}>(autogenerated)</{tag}>")
    }

    fn describe(&self) -> Result<String> {
        match self {
            Self::RustdocLinkOptions => {
                describe_options::<mdbookkit::bin::rustdoc_link::env::Config>()
            }
            Self::LinkForeverOptions => describe_options::<mdbookkit::bin::link_forever::Config>(),
        }
    }
}

fn main() -> Result<()> {
    let ty = match Program::parse() {
        Program {
            command: Some(Command::Supports { .. }),
            ..
        } => return Ok(()),

        Program { reflect, .. } => reflect,
    };

    let (_, mut book): (PreprocessorContext, Book) = Vec::new()
        .pipe(|mut buf| std::io::stdin().read_to_end(&mut buf).and(Ok(buf)))?
        .pipe(String::from_utf8)?
        .pipe_as_ref(serde_json::from_str)?;

    let content = ty.describe()?;

    let tag = ty.tag();

    book.for_each_mut(|page| {
        let BookItem::Chapter(page) = page else {
            return;
        };
        page.content = page.content.replace(&tag, &content);
    });

    let output = serde_json::to_string(&book)?;
    std::io::stdout().write_all(output.as_bytes())?;
    Ok(())
}

#[derive(Serialize)]
struct OptionItem {
    key: String,
    help: String,
    description: String,
    type_id: Option<String>,
    default: Option<String>,
    choices: Vec<(String, String)>,
}

fn describe_options<C: CommandFactory>() -> Result<String> {
    let template = r#"
<div class="table-wrapper">
<table>

<thead>
<tr>
<td>Option</td>
<td>Summary</td>
</tr>
</thead>

<tbody>

{% for option in options %}

<tr>
<td style="text-align: left; white-space: nowrap;">

[`{{ option.key }}`](#{{ option.key }})

</td>
<td>

{{ option.help }}

</td>
</tr>

{% endfor %}

</tbody>

</table>
</div>

{% for option in options -%}

## `{{ option.key }}`

{{ option.description }}

{% if option.choices %}
<div class="table-wrapper">
<table>

<thead>
<tr>
<td>Choice</td>
<td>Description</td>
</tr>
</thead>

<tbody>

{% for choice, description in option.choices %}
<tr>

<td style="text-align: left;">
<code>{{ choice }}</code>
</td>

<td>

{{ description }}

</td>

</tr>
{% endfor %}

</tbody>

</table>
</div>
{% endif %}

<div class="table-wrapper">
<table>
<tbody>

{%- if option.default -%}

<tr>
<th style="text-align: left;">Default</th>
<td>

`{{ option.default }}`

</td>
</tr>

{%- endif -%}

{%- if option.type_id -%}

<tr>
<th style="text-align: left;">Type</th>
<td>

[`{{ option.type_id }}`]

</td>
</tr>

{%- endif -%}

</tbody>
</table>
</div>

{% endfor -%}
"#;

    let options = C::command()
        .get_opts()
        .filter(|opt| !opt.is_hide_set())
        .map(|opt| {
            let key = opt.get_long().unwrap().to_owned();

            let help = opt.get_help().map(|h| h.to_string()).unwrap_or_default();

            let description = opt
                .get_long_help()
                .map(|h| h.to_string())
                .unwrap_or_default();

            let action = opt.get_action();

            let type_id = if cfg!(debug_assertions) {
                let ty = format!("{:?}", opt.get_value_parser().type_id())
                    .replace("alloc::string::", "");
                if matches!(action, ArgAction::Append) {
                    Some(format!("Vec<{ty}>"))
                } else {
                    Some(ty)
                }
            } else {
                None
            };

            let default = if let Some(d) = opt.get_default_values().iter().next() {
                Some(format!("{:?}", d.to_string_lossy().into_owned()))
            } else if matches!(action, ArgAction::SetTrue) {
                Some("false".into())
            } else if matches!(action, ArgAction::SetFalse) {
                Some("true".into())
            } else if matches!(action, ArgAction::Append) {
                Some("[]".into())
            } else if !opt.is_required_set() {
                Some("None".into())
            } else {
                None
            };

            let choices = opt
                .get_possible_values()
                .iter()
                .filter_map(|v| {
                    if v.is_hide_set() {
                        None
                    } else {
                        let help = v.get_help()?;
                        Some((format!("{:?}", v.get_name()), help.to_string()))
                    }
                })
                .collect();

            OptionItem {
                key,
                help,
                description,
                type_id,
                default,
                choices,
            }
        })
        .collect::<Vec<_>>();

    Ok(render!(template, options))
}
