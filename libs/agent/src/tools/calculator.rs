use std::sync::Arc;

use crate::AgentConfig;
use crate::Tool;
use crate::ToolDesc;
use async_trait::async_trait;
use llm::{Function, Tool as LlmTool};
use serde_json::{Value, json};

pub struct CalculatorTool;

#[derive(ToolDesc)]
struct CalculatorParams {
    /// Arithmetic expression to evaluate. Supports +, -, *, / and parentheses.
    /// Example: "(9 + 46) / 34"
    expression: String,
}

#[async_trait(?Send)]
impl Tool for CalculatorTool {
    fn name(&self) -> &str {
        "calculator"
    }

    fn readable_name(&self) -> &str {
        "Calculator"
    }

    fn definition(&self) -> LlmTool {
        LlmTool {
            r#type: llm::ToolType::Function,
            function: Function {
                description: "Evaluate a basic arithmetic expression (+, -, *, /, parentheses)."
                    .to_string(),
                name: self.name().to_owned(),
                parameters: Some(CalculatorParams::function_parameters()),
            },
        }
    }

    async fn execute(&self, _config: Arc<AgentConfig>, args: Value) -> Value {
        let args = CalculatorParams::decode(args).unwrap();
        match eval(&args.expression) {
            Ok(result) => json!({ "result": result }),
            Err(e) => json!({ "error": e }),
        }
    }
}

fn eval(input: &str) -> Result<f64, String> {
    let tokens: Vec<char> = input.chars().filter(|c| !c.is_whitespace()).collect();
    let (val, pos) = parse_expr(&tokens, 0)?;
    if pos != tokens.len() {
        return Err(format!("unexpected character '{}'", tokens[pos]));
    }
    Ok(val)
}

fn parse_expr(tokens: &[char], pos: usize) -> Result<(f64, usize), String> {
    let (mut val, mut pos) = parse_term(tokens, pos)?;
    while pos < tokens.len() && (tokens[pos] == '+' || tokens[pos] == '-') {
        let op = tokens[pos];
        let (rhs, next) = parse_term(tokens, pos + 1)?;
        val = if op == '+' { val + rhs } else { val - rhs };
        pos = next;
    }
    Ok((val, pos))
}

fn parse_term(tokens: &[char], pos: usize) -> Result<(f64, usize), String> {
    let (mut val, mut pos) = parse_factor(tokens, pos)?;
    while pos < tokens.len() && (tokens[pos] == '*' || tokens[pos] == '/') {
        let op = tokens[pos];
        let (rhs, next) = parse_factor(tokens, pos + 1)?;
        if op == '/' && rhs == 0.0 {
            return Err("division by zero".to_string());
        }
        val = if op == '*' { val * rhs } else { val / rhs };
        pos = next;
    }
    Ok((val, pos))
}

fn parse_factor(tokens: &[char], pos: usize) -> Result<(f64, usize), String> {
    if pos >= tokens.len() {
        return Err("unexpected end of expression".to_string());
    }
    if tokens[pos] == '(' {
        let (val, pos) = parse_expr(tokens, pos + 1)?;
        if pos >= tokens.len() || tokens[pos] != ')' {
            return Err("missing closing parenthesis".to_string());
        }
        return Ok((val, pos + 1));
    }
    parse_number(tokens, pos)
}

fn parse_number(tokens: &[char], pos: usize) -> Result<(f64, usize), String> {
    let mut end = pos;
    if end < tokens.len() && tokens[end] == '-' {
        end += 1;
    }
    if end >= tokens.len() || !tokens[end].is_ascii_digit() {
        return Err(format!("expected number at position {}", pos));
    }
    while end < tokens.len() && tokens[end].is_ascii_digit() {
        end += 1;
    }
    if end < tokens.len() && tokens[end] == '.' {
        end += 1;
        while end < tokens.len() && tokens[end].is_ascii_digit() {
            end += 1;
        }
    }
    let s: String = tokens[pos..end].iter().collect();
    let val: f64 = s.parse().map_err(|_| format!("invalid number '{s}'"))?;
    Ok((val, end))
}
