use friday_agent::{AgentConfig, ModelRegistry, Tool, tools::calculator::CalculatorTool};
use serde_json::json;
use std::sync::Arc;

fn dummy_config() -> Arc<AgentConfig> {
    Arc::new(AgentConfig {
        model_registry: ModelRegistry::new(),
        max_iterations: 50,
    })
}

fn calc(expr: &str) -> serde_json::Value {
    futures::executor::block_on(
        CalculatorTool.execute(dummy_config(), json!({ "expression": expr })),
    )
}

#[test]
fn addition() {
    assert_eq!(calc("1 + 2"), json!({ "result": 3.0 }));
}

#[test]
fn subtraction() {
    assert_eq!(calc("10 - 4"), json!({ "result": 6.0 }));
}

#[test]
fn multiplication() {
    assert_eq!(calc("3 * 7"), json!({ "result": 21.0 }));
}

#[test]
fn division() {
    assert_eq!(calc("(9 + 46) / 34"), json!({ "result": 55.0 / 34.0 }));
}

#[test]
fn operator_precedence() {
    assert_eq!(calc("2 + 3 * 4"), json!({ "result": 14.0 }));
}

#[test]
fn nested_parentheses() {
    assert_eq!(calc("(2 + (3 * 4))"), json!({ "result": 14.0 }));
}

#[test]
fn division_by_zero() {
    assert_eq!(calc("1 / 0"), json!({ "error": "division by zero" }));
}
