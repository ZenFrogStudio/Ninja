use serde::Deserialize;

#[cfg(any(target_os = "macos", windows))]
use super::komorebi::KomorebiProviderConfig;
#[cfg(windows)]
use super::{
  audio::AudioProviderConfig, keyboard::KeyboardProviderConfig,
  media::MediaProviderConfig, systray::SystrayProviderConfig,
};
use super::{
  battery::BatteryProviderConfig, cpu::CpuProviderConfig,
  disk::DiskProviderConfig, host::HostProviderConfig,
  memory::MemoryProviderConfig, network::NetworkProviderConfig,
  ninja::NinjaProviderConfig,
};

#[derive(Deserialize, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProviderConfig {
  #[cfg(windows)]
  Audio(AudioProviderConfig),
  Battery(BatteryProviderConfig),
  Cpu(CpuProviderConfig),
  Host(HostProviderConfig),
  // `glazewm` is still accepted so widget packs written against the
  // upstream bar keep loading.
  #[serde(rename = "ninja", alias = "glazewm")]
  Ninja(NinjaProviderConfig),
  #[cfg(any(target_os = "macos", windows))]
  Komorebi(KomorebiProviderConfig),
  #[cfg(windows)]
  Media(MediaProviderConfig),
  Memory(MemoryProviderConfig),
  Disk(DiskProviderConfig),
  Network(NetworkProviderConfig),
  #[cfg(windows)]
  Systray(SystrayProviderConfig),
  #[cfg(windows)]
  Keyboard(KeyboardProviderConfig),
}
