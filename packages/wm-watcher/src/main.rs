// The `windows` or `console` subsystem (default is `console`) determines
// whether a console window is spawned on launch, if not already ran
// through a console. The following prevents this additional console window
// in release mode.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![warn(clippy::all, clippy::pedantic)]
// `CLAUDE.md` requires a `SAFETY:` comment on every `unsafe` block.
#![warn(clippy::undocumented_unsafe_blocks)]

use std::{process, sync::mpsc, thread, time::Duration};

use anyhow::Context;
use wm_common::{ClientResponseData, ContainerDto, WindowDto, WmEvent};
use wm_ipc_client::IpcClient;
use wm_platform::{NativeWindow, NativeWindowWindowsExt, OpacityValue};

/// Upper bound on how long window restoration may take.
///
/// The watcher exists to tidy up after a crashed WM; it must never become
/// a lingering process itself.
const CLEANUP_BUDGET: Duration = Duration::from_secs(5);

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  tracing_subscriber::fmt().init();

  let mut client = IpcClient::connect().await?;

  // Get handles to windows that are already open on watcher launch.
  let mut managed_handles = query_initial_windows(&mut client)
    .await?
    .into_iter()
    .map(|window| window.handle)
    .collect::<Vec<_>>();

  // Update window handles on window manage/unmanage events.
  let subscribe_res =
    watch_managed_handles(&mut client, &mut managed_handles).await;

  match subscribe_res {
    Ok(()) => {
      tracing::info!("WM exited successfully. Skipping watcher cleanup.");
    }
    Err(err) => {
      tracing::info!(
        "Running watcher cleanup. WM exited unexpectedly: {}",
        err
      );

      // Handles are captured over the WM's lifetime, so some will refer
      // to windows that have since been destroyed. Operating on those is
      // at best wasted work and at worst a blocking call into a process
      // that is itself shutting down.
      let managed_windows = managed_handles
        .into_iter()
        .map(NativeWindow::from_handle)
        .filter(NativeWindow::is_valid)
        .collect::<Vec<_>>();

      // Restoration runs on its own thread so that `CLEANUP_BUDGET` is a
      // real bound. Most of the work goes through COM, which blocks with
      // no timeout when a window's owning process is unresponsive, and a
      // deadline can only be checked between windows — never during a call
      // already in progress. Watchers were outliving the WM by hours this
      // way, holding ~16MB each and leaving the next session unprotected.
      let (done_tx, done_rx) = mpsc::channel();

      thread::spawn(move || {
        restore_windows(&managed_windows);
        let _ = done_tx.send(());
      });

      if done_rx.recv_timeout(CLEANUP_BUDGET).is_ok() {
        tracing::info!("Watcher cleanup complete.");
      } else {
        tracing::warn!(
          "Cleanup budget exhausted; some windows may still be hidden."
        );
      }

      // The restoration thread may still be wedged inside a COM call that
      // can't be cancelled, so returning from `main` wouldn't be enough to
      // end the process.
      process::exit(0);
    }
  }

  Ok(())
}

/// Returns windows to a state the user can reach.
///
/// Ordered so that the work which rescues a window comes before the work
/// which merely tidies it, since a call that wedges stops everything after
/// it. Cheap non-blocking calls run for every window first, then
/// uncloaking, then cosmetics.
fn restore_windows(windows: &[NativeWindow]) {
  // `show` is a non-blocking `ShowWindowAsync`.
  for window in windows {
    if let Err(err) = window.show() {
      tracing::warn!("Failed to show window: {:?}", err);
    }
  }

  // Uncloaking decides whether a window can be found at all: one left
  // cloaked is absent from the taskbar and the task switcher alike, and
  // the WM won't reclaim it on the next run either, since a cloaked window
  // reports itself as not visible.
  for window in windows {
    if let Err(err) = window.set_cloaked(false) {
      tracing::warn!("Failed to uncloak window: {:?}", err);
    }

    let _ = window.set_taskbar_visibility(true);
  }

  // Cosmetic, so it runs last.
  for window in windows {
    let _ = window.set_border_color(None);
    let _ = window.set_transparency(&OpacityValue::from_alpha(u8::MAX));
  }
}

async fn query_initial_windows(
  client: &mut IpcClient,
) -> anyhow::Result<Vec<WindowDto>> {
  let query_message = "query windows";

  client
    .send(query_message)
    .await
    .context("Failed to send window query command.")?;

  client
    .client_response(query_message)
    .await
    .and_then(|response| match response.data {
      Some(ClientResponseData::Windows(data)) => Some(data),
      _ => None,
    })
    .map(|data| {
      data
        .windows
        .into_iter()
        .filter_map(|container| match container {
          ContainerDto::Window(window) => Some(window),
          _ => None,
        })
        .collect::<Vec<_>>()
    })
    .context("Invalid data in windows query response.")
}

async fn watch_managed_handles(
  client: &mut IpcClient,
  handles: &mut Vec<isize>,
) -> anyhow::Result<()> {
  let subscription_message =
    "sub -e window_managed window_unmanaged application_exiting";

  client
    .send(subscription_message)
    .await
    .context("Failed to send subscribe command to IPC server.")?;

  let subscription_id = client
    .client_response(subscription_message)
    .await
    .and_then(|response| match response.data {
      Some(ClientResponseData::EventSubscribe(data)) => {
        Some(data.subscription_id)
      }
      _ => None,
    })
    .context("No subscription ID in watcher event subscription.")?;

  loop {
    let event_data = client
      .event_subscription(&subscription_id)
      .await
      .and_then(|event| event.data);

    match event_data {
      Some(WmEvent::WindowManaged { managed_window }) => {
        if let ContainerDto::Window(window) = managed_window {
          tracing::info!("Watcher added handle: {}.", window.handle);
          handles.push(window.handle);
        }
      }
      Some(WmEvent::WindowUnmanaged {
        unmanaged_handle, ..
      }) => {
        tracing::info!("Watcher removed handle: {}.", unmanaged_handle);
        handles.retain(|&handle| handle != unmanaged_handle);
      }
      Some(WmEvent::ApplicationExiting) => {
        return Ok(());
      }
      Some(_) => unreachable!(),
      None => {
        anyhow::bail!("IPC connection closed unexpectedly.")
      }
    }
  }
}
