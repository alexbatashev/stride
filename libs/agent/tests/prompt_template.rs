use stride_agent::build_prompt;

#[test]
fn prompt_template_interpolates_expressions() {
    let name = "Ada";
    let count = 3;

    let prompt = build_prompt!("Hello {name}, count is {count + 1}.");

    assert_eq!(prompt, "Hello Ada, count is 4.");
}

#[test]
fn prompt_template_renders_conditionals() {
    let enabled = true;
    let prompt = build_prompt!("Mode: {if enabled}on{else}off{/if}");

    assert_eq!(prompt, "Mode: on");
}

#[test]
fn prompt_template_renders_loops_and_escaped_braces() {
    let items = ["alpha", "beta"];

    let prompt = build_prompt!("Items: {for item in items.iter()}{item}; {/for}{{done}}");

    assert_eq!(prompt, "Items: alpha; beta; {done}");
}
