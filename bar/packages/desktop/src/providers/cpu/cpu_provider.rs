use serde::{Deserialize, Serialize};
use sysinfo::System;

use crate::{
  common::SyncInterval,
  providers::{
    CommonProviderState, Provider, ProviderInputMsg, RuntimeType,
  },
};

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CpuProviderConfig {
  pub refresh_interval: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CpuOutput {
  pub frequency: u64,
  pub usage: f32,
  pub logical_core_count: usize,
  pub physical_core_count: usize,
  pub vendor: String,
}

pub struct CpuProvider {
  config: CpuProviderConfig,
  common: CommonProviderState,
}

impl CpuProvider {
  pub fn new(
    config: CpuProviderConfig,
    common: CommonProviderState,
  ) -> CpuProvider {
    CpuProvider { config, common }
  }

  fn run_interval(&self) -> anyhow::Result<CpuOutput> {
    let mut sysinfo = self.common.sysinfo.blocking_lock();
    sysinfo.refresh_cpu_all();

    // All cores share the same physical package, so the first core's
    // frequency and vendor stand in for the "global" value that older
    // `sysinfo` versions exposed via `global_cpu_info`.
    let first_cpu = sysinfo.cpus().first();

    Ok(CpuOutput {
      usage: sysinfo.global_cpu_usage(),
      frequency: first_cpu.map(|cpu| cpu.frequency()).unwrap_or(0),
      logical_core_count: sysinfo.cpus().len(),
      physical_core_count: System::physical_core_count()
        .unwrap_or(sysinfo.cpus().len()),
      vendor: first_cpu.map(|cpu| cpu.vendor_id()).unwrap_or("").into(),
    })
  }
}

impl Provider for CpuProvider {
  fn runtime_type(&self) -> RuntimeType {
    RuntimeType::Sync
  }

  fn start_sync(&mut self) {
    let mut interval = SyncInterval::new(self.config.refresh_interval);

    loop {
      crossbeam::select! {
        recv(interval.tick()) -> _ => {
          let output = self.run_interval();
          self.common.emitter.emit_output(output);
        }
        recv(self.common.input.sync_rx) -> input => {
          if let Ok(ProviderInputMsg::Stop) = input {
            break;
          }
        }
      }
    }
  }
}
