// SPDX-FileCopyrightText: 2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

use anyhow::{Context, Result, ensure};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::{Child, ChildStdout, Command};
use tokio::task::JoinSet;
use tonic::async_trait;

use super::pipeline::{CommandSpec, Pipeline};
use super::plan::Plan;

#[async_trait]
pub(crate) trait Executor {
    async fn run_pipeline(&self, pipeline: &Pipeline) -> Result<()>;

    async fn run_plan(&self, plan: &Plan) -> Result<()> {
        for pipeline in &plan.steps {
            self.run_pipeline(pipeline).await?;
        }
        Ok(())
    }
}

pub struct DryRunExecutor;

#[async_trait]
impl Executor for DryRunExecutor {
    async fn run_pipeline(&self, pipeline: &Pipeline) -> Result<()> {
        println!("DRY-RUN: {}", pipeline.format_shell());
        Ok(())
    }
}

/// Executes every pipeline stage directly and waits for every child.
///
/// This intentionally does not invoke a shell: arguments are never reparsed,
/// and a decompressor failure cannot be hidden by a successful trailing `dd`.
pub struct ShellExecutor;

fn command(spec: &CommandSpec) -> Command {
    let mut command = Command::new(&spec.program);
    command.args(&spec.args);
    command
}

async fn terminate(children: &mut [(String, Child)]) {
    for (_, child) in children {
        let _ = child.kill().await;
    }
}

#[async_trait]
impl Executor for ShellExecutor {
    async fn run_pipeline(&self, pipeline: &Pipeline) -> Result<()> {
        ensure!(
            !pipeline.is_empty(),
            "refusing to execute an empty pipeline"
        );
        if pipeline.is_sequential() {
            for stage in pipeline.stages() {
                run_stages(std::slice::from_ref(stage)).await?;
            }
            return Ok(());
        }
        run_stages(pipeline.stages()).await
    }
}

async fn run_stages(stages: &[CommandSpec]) -> Result<()> {
    let mut children: Vec<(String, Child)> = Vec::new();
    let mut previous: Option<ChildStdout> = None;
    let mut pumps = JoinSet::new();
    let last = stages.len() - 1;

    for (index, spec) in stages.iter().enumerate() {
        let mut child_command = command(spec);
        child_command.stderr(Stdio::inherit());
        if index == 0 {
            child_command.stdin(Stdio::inherit());
        } else {
            child_command.stdin(Stdio::piped());
        }
        if index == last {
            child_command.stdout(Stdio::inherit());
        } else {
            child_command.stdout(Stdio::piped());
        }

        let description = Pipeline::new(spec.clone()).format_shell();
        let mut child = match child_command.spawn() {
            Ok(child) => child,
            Err(error) => {
                terminate(&mut children).await;
                return Err(error).with_context(|| format!("spawning {description}"));
            }
        };
        if index > 0 {
            let mut stdout = previous
                .take()
                .context("previous pipeline child stdout was not piped")?;
            let mut stdin = child
                .stdin
                .take()
                .context("pipeline child stdin was not piped")?;
            pumps.spawn(async move {
                tokio::io::copy(&mut stdout, &mut stdin).await?;
                stdin.shutdown().await
            });
        }
        previous = child.stdout.take();
        children.push((description, child));
    }

    let mut failure = None;
    for (description, child) in &mut children {
        let status = child
            .wait()
            .await
            .with_context(|| format!("waiting for {description}"))?;
        if !status.success() && failure.is_none() {
            failure = Some(anyhow::anyhow!(
                "command failed (exit={}): {description}",
                status.code().unwrap_or(-1)
            ));
        }
    }
    while let Some(pump) = pumps.join_next().await {
        pump.context("pipeline copy task failed")??;
    }
    if let Some(error) = failure {
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_failure_from_non_final_pipeline_stage() {
        let pipeline = Pipeline::new(CommandSpec::new("sh").args(["-c", "exit 19"]))
            .pipe(CommandSpec::new("sh").args(["-c", "cat >/dev/null; exit 0"]));
        let error = ShellExecutor.run_pipeline(&pipeline).await.unwrap_err();
        assert!(error.to_string().contains("exit=19"));
    }
}
