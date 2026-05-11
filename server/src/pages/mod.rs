pub mod agent;
pub mod auth;

use handlebars::Handlebars;

pub fn get_templates() -> anyhow::Result<Handlebars<'static>> {
    let mut handlebars = Handlebars::new();

    handlebars.register_template_string("base", BASE_TEMPLATE)?;

    Ok(handlebars)
}

const BASE_TEMPLATE: &str = "<!doctype html>
<html>
    <head>
		<meta charset=\"utf-8\" />
		<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\" />
        <title>{{title}}</title>
    </head>
    <body>
        {{> page}}
    </body>
</html>";
