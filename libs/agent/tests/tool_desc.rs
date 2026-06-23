use serde_json::json;
use stride_agent::ToolDesc;

#[derive(Debug, PartialEq, ToolDesc)]
struct SearchArgs {
    /// Search query text.
    query: String,
    /// Maximum number of results.
    limit: Option<u32>,
    /// Include hidden entries.
    include_hidden: bool,
}

#[derive(Debug, PartialEq, ToolDesc)]
struct OptionalArgs {
    /// Optional query.
    query: Option<String>,
}

#[test]
fn derives_function_parameters() {
    let params = SearchArgs::function_parameters();
    let params_from_into: llm::FunctionParameters = SearchArgs {
        query: String::new(),
        limit: None,
        include_hidden: false,
    }
    .into();

    assert_eq!(params.param_type, "object");
    assert_eq!(params_from_into.param_type, "object");
    assert_eq!(
        params.required,
        Some(vec!["query".to_string(), "include_hidden".to_string()])
    );

    let query = params.properties.get("query").unwrap();
    assert_eq!(query.r#type, "string");
    assert_eq!(query.description, "Search query text.");

    let limit = params.properties.get("limit").unwrap();
    assert_eq!(limit.r#type, "integer");
    assert_eq!(limit.description, "Maximum number of results.");

    let include_hidden = params.properties.get("include_hidden").unwrap();
    assert_eq!(include_hidden.r#type, "boolean");
    assert_eq!(include_hidden.description, "Include hidden entries.");
}

#[test]
fn decodes_from_json_value() {
    let args = SearchArgs::decode(json!({
        "query": "needle",
        "include_hidden": true
    }))
    .unwrap();

    assert_eq!(
        args,
        SearchArgs {
            query: "needle".to_string(),
            limit: None,
            include_hidden: true,
        }
    );
}

#[test]
fn reports_missing_required_field() {
    let err = SearchArgs::try_from(json!({ "include_hidden": true })).unwrap_err();

    assert_eq!(err, "missing required parameter `query`");
}

#[test]
fn supports_structs_with_only_optional_fields() {
    let params = OptionalArgs::function_parameters();
    assert_eq!(params.required, None);
    assert_eq!(
        OptionalArgs::decode(json!({})).unwrap(),
        OptionalArgs { query: None }
    );
}
