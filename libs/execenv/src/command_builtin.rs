use std::sync::Arc;

use bashkit::{Builtin, BuiltinContext, ExecResult, async_trait};

use crate::{CommandRouter, ExecInvocation};

pub struct CommandBuiltin {
    router: Arc<CommandRouter>,
    name: &'static str,
    description: &'static str,
    inline: bool,
}

impl CommandBuiltin {
    pub fn new(router: Arc<CommandRouter>, name: &'static str, description: &'static str) -> Self {
        Self {
            router,
            name,
            description,
            inline: false,
        }
    }

    pub(crate) fn new_inline(
        router: Arc<CommandRouter>,
        name: &'static str,
        description: &'static str,
    ) -> Self {
        Self {
            router,
            name,
            description,
            inline: true,
        }
    }
}

#[async_trait]
impl Builtin for CommandBuiltin {
    async fn execute(&self, ctx: BuiltinContext<'_>) -> bashkit::Result<ExecResult> {
        let mut argv = Vec::with_capacity(ctx.args.len() + 1);
        argv.push(self.name.to_string());
        argv.extend(ctx.args.iter().cloned());
        let invocation = ExecInvocation {
            argv,
            stdin: ctx.stdin.unwrap_or_default().as_bytes().to_vec(),
            cwd: ctx.cwd.to_string_lossy().into_owned(),
            timeout: None,
        };
        let output = if self.inline {
            self.router
                .exec_inline(invocation)
                .await
                .unwrap_or_else(|error| crate::CommandOutput {
                    returncode: 1,
                    stderr: format!("{}: {error:#}\n", self.name).into_bytes(),
                    ..Default::default()
                })
        } else {
            self.router.exec(invocation).await
        };
        Ok(ExecResult {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.returncode,
            ..Default::default()
        })
    }

    fn llm_hint(&self) -> Option<&'static str> {
        Some(self.description)
    }
}
