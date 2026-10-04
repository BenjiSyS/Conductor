use std::path::PathBuf;

use async_trait::async_trait;
use conductor_orchestrator::goal::CheckResult;
use conductor_orchestrator::runner::Verifier;
use tokio_util::sync::CancellationToken;

/// Runs Definition-of-Done commands in the project folder.
pub struct GateVerifier {
    pub root: PathBuf,
    pub timeout_secs: u64,
}

#[async_trait]
impl Verifier for GateVerifier {
    async fn run_check(&self, command: &str, cancel: CancellationToken) -> CheckResult {
        let r = conductor_tools::testgate::run_gate(command, &self.root, self.timeout_secs, cancel)
            .await;
        CheckResult {
            check: command.to_string(),
            passed: r.passed,
            evidence: r.evidence,
        }
    }
}
