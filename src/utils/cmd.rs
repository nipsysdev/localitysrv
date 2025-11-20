use std::path::Path;
use std::process::Command;
use thiserror::Error;
use tokio::process::Command as TokioCommand;

#[derive(Error, Debug)]
pub enum CmdError {
    #[error("Command not found: {0}")]
    CommandNotFound(String),
    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Command exited with non-zero status: {0}")]
    NonZeroExit(i32),
}

pub async fn is_tool_available(tool: &str) -> bool {
    Command::new(tool)
        .arg("--help")
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}

pub async fn ensure_tools_are_present(tools: &[&str]) -> Result<(), CmdError> {
    let mut missing_tools = Vec::new();

    for &tool in tools {
        if !is_tool_available(tool).await {
            missing_tools.push(tool);
        }
    }

    if !missing_tools.is_empty() {
        return Err(CmdError::CommandNotFound(format!(
            "Required tools not found: {}",
            missing_tools.join(", ")
        )));
    }

    Ok(())
}

pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
}

pub async fn run_command(
    command: &str,
    args: &[&str],
    working_dir: Option<&Path>,
) -> Result<CommandOutput, CmdError> {
    let mut cmd = TokioCommand::new(command);

    cmd.args(args);

    if let Some(dir) = working_dir {
        cmd.current_dir(dir);
    }

    let output = cmd.output().await?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).to_string();

    if !output.status.success() {
        return Err(CmdError::NonZeroExit(output.status.code().unwrap_or(-1)));
    }

    Ok(CommandOutput { stdout, stderr })
}
