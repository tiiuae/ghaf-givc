// SPDX-FileCopyrightText: 2025-2026 TII (SSRC) and the Ghaf contributors
// SPDX-License-Identifier: Apache-2.0

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    givc_agent::trace_init()?;
    givc_agent::runtime::AgentRuntime::default().serve().await
}
