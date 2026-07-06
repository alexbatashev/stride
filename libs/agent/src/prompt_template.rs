#[macro_export]
macro_rules! prompt_template {
    ($template:literal $(, $($args:tt)*)?) => {{
        format!($template $(, $($args)*)?)
    }};
}

#[cfg(test)]
mod tests {
    #[test]
    fn renders_with_compile_checked_format_args() {
        let name = "Stride";
        let rendered = prompt_template!("Hello {name}.");

        assert_eq!(rendered, "Hello Stride.");
    }
}
