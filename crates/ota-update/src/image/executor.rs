use anyhow::Result;
use async_trait::async_trait;
use std::process::Stdio;
use tokio::process::Command;

use super::pipeline::Pipeline;
use super::plan::Plan;

#[async_trait]
pub trait Executor {
    async fn run_pipeline(&self, pipeline: &Pipeline) -> Result<()>;

    async fn run_plan(&self, plan: &Plan) -> Result<()> {
        for pipeline in &plan.steps {
            self.run_pipeline(&pipeline).await?;
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

pub struct ShellExecutor {
    pub shell: String,
}

impl Default for ShellExecutor {
    fn default() -> Self {
        Self {
            shell: "/bin/sh".into(),
        }
    }
}

#[async_trait]
impl Executor for ShellExecutor {
    async fn run_pipeline(&self, pipeline: &Pipeline) -> Result<()> {
        let cmdline = pipeline.format_shell();

        let status = Command::new(&self.shell)
            .arg("-c")
            .arg(&cmdline)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()
            .await?;

        if !status.success() {
            anyhow::bail!(
                "pipeline failed (exit={}): {}",
                status.code().unwrap_or(-1),
                cmdline
            );
        }

        Ok(())
    }
}
