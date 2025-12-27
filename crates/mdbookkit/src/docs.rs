use std::any::TypeId;

use anyhow::Result;
use clap::{ArgAction, CommandFactory, builder::ValueParser};
use minijinja::render;
use serde::Serialize;
use tap::Tap;

// TODO: define options using facet

#[derive(Debug, Default)]
pub struct Reflect {
    type_map: Vec<(TypeId, &'static str)>,
}

impl Reflect {
    pub fn map_type<T: 'static>(&mut self, repl: &'static str) -> &mut Self {
        self.type_map.push((TypeId::of::<T>(), repl));
        self
    }

    fn get_type(&self, opt: &ValueParser) -> String {
        let source = opt.type_id();
        self.type_map
            .iter()
            .find_map(|(id, mapped)| {
                if source == *id {
                    Some((*mapped).into())
                } else {
                    None
                }
            })
            .unwrap_or_else(|| format!("{source:?}"))
    }

    pub fn describe<C: CommandFactory>(&self) -> Result<String> {
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
<td style="text-align: left;">

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

[`{{ option.type_id[0] }}`][{{ option.type_id[1] }}]

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
                let key = opt
                    .get_long()
                    .expect("option should have a long name")
                    .to_owned();

                let help = opt.get_help().map(|h| h.to_string()).unwrap_or_default();

                let description = opt
                    .get_long_help()
                    .map(|h| h.to_string())
                    .unwrap_or(help.clone());

                let action = opt.get_action();

                let type_id = if cfg!(debug_assertions) {
                    let ty = self
                        .get_type(opt.get_value_parser())
                        .replace("alloc::string::", "");
                    let name = ty.split("::").last().expect("split() shouldn't be empty");
                    if matches!(action, ArgAction::Append) {
                        Some((format!("Vec<{name}>"), format!("Vec<{ty}>")))
                    } else {
                        Some((name.to_owned(), ty))
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

                #[derive(Serialize)]
                struct OptionItem {
                    key: String,
                    help: String,
                    description: String,
                    type_id: Option<(String, String)>,
                    default: Option<String>,
                    choices: Vec<(String, String)>,
                }

                OptionItem {
                    key,
                    help,
                    description,
                    type_id,
                    default,
                    choices,
                }
            })
            .collect::<Vec<_>>()
            .tap_mut(|opts| opts.sort_by(|a, b| a.key.cmp(&b.key)));

        Ok(render!(template, options))
    }
}
