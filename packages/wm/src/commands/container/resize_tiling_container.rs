use crate::{
  models::TilingContainer,
  traits::{CommonGetters, TilingSizeGetters, MIN_TILING_SIZE},
};

pub fn resize_tiling_container(
  container_to_resize: &TilingContainer,
  target_size: f32,
) {
  let tiling_siblings =
    container_to_resize.tiling_siblings().collect::<Vec<_>>();

  // Ignore cases where the container is the only child.
  if tiling_siblings.is_empty() {
    container_to_resize.set_tiling_size(1.);
    return;
  }

  if !target_size.is_finite() {
    return;
  }

  // Prevent the container from being smaller than the minimum size, and
  // larger than the space available from sibling containers.
  #[allow(clippy::cast_precision_loss)]
  let max_target_size = (1.
    - (tiling_siblings.len() as f32 * MIN_TILING_SIZE))
    .max(MIN_TILING_SIZE);
  let clamped_target_size =
    target_size.clamp(MIN_TILING_SIZE, max_target_size);

  container_to_resize.set_tiling_size(clamped_target_size);
  distribute_tiling_size(&tiling_siblings, 1.0 - clamped_target_size);
}

pub(super) fn distribute_tiling_size(
  containers: &[TilingContainer],
  total_size: f32,
) {
  if containers.is_empty() || !total_size.is_finite() {
    return;
  }

  #[allow(clippy::cast_precision_loss)]
  let container_count = containers.len() as f32;
  let distributable_size =
    (total_size - (container_count * MIN_TILING_SIZE)).max(0.0);

  let available_size = containers
    .iter()
    .fold(0.0, |sum, container| sum + available_tiling_size(container));

  for container in containers {
    let size = if available_size > f32::EPSILON {
      let available = available_tiling_size(container);
      MIN_TILING_SIZE + ((available / available_size) * distributable_size)
    } else {
      total_size / container_count
    };

    container.set_tiling_size(size);
  }
}

fn available_tiling_size(container: &TilingContainer) -> f32 {
  let available = container.tiling_size() - MIN_TILING_SIZE;
  if available.is_finite() {
    available.max(0.0)
  } else {
    0.0
  }
}

#[cfg(test)]
mod tests {
  use super::resize_tiling_container;
  use crate::{
    models::{TilingContainer, TilingWindow, Workspace},
    traits::{TilingSizeGetters, MIN_TILING_SIZE},
  };

  #[test]
  fn redistributes_size_when_all_siblings_are_at_minimum() {
    let resized = TilingWindow::mock().call();
    let sibling_a = TilingWindow::mock().call();
    let sibling_b = TilingWindow::mock().call();
    let workspace = Workspace::mock()
      .tiling_containers(vec![
        resized.clone().into(),
        sibling_a.clone().into(),
        sibling_b.clone().into(),
      ])
      .call();

    resized.set_tiling_size(1.0 - (2.0 * MIN_TILING_SIZE));
    sibling_a.set_tiling_size(MIN_TILING_SIZE);
    sibling_b.set_tiling_size(MIN_TILING_SIZE);

    resize_tiling_container(&TilingContainer::from(resized.clone()), 0.5);

    let sizes = [
      resized.tiling_size(),
      sibling_a.tiling_size(),
      sibling_b.tiling_size(),
    ];
    assert!(sizes.iter().all(|size| size.is_finite()));
    assert!((sizes.iter().sum::<f32>() - 1.0).abs() < f32::EPSILON);
    assert!((sibling_a.tiling_size() - 0.25).abs() < f32::EPSILON);
    assert!((sibling_b.tiling_size() - 0.25).abs() < f32::EPSILON);

    drop(workspace);
  }

  #[test]
  fn ignores_non_finite_target_sizes() {
    let resized = TilingWindow::mock().tiling_size(0.5).call();
    let sibling = TilingWindow::mock().tiling_size(0.5).call();
    let workspace = Workspace::mock()
      .tiling_containers(vec![
        resized.clone().into(),
        sibling.clone().into(),
      ])
      .call();

    resize_tiling_container(&resized.clone().into(), f32::NAN);

    assert_eq!(resized.tiling_size(), 0.5);
    assert_eq!(sibling.tiling_size(), 0.5);

    drop(workspace);
  }

  #[test]
  fn repairs_non_finite_sibling_sizes() {
    let resized = TilingWindow::mock().call();
    let invalid_sibling = TilingWindow::mock().call();
    let valid_sibling = TilingWindow::mock().call();
    let workspace = Workspace::mock()
      .tiling_containers(vec![
        resized.clone().into(),
        invalid_sibling.clone().into(),
        valid_sibling.clone().into(),
      ])
      .call();

    resized.set_tiling_size(0.5);
    invalid_sibling.set_tiling_size(f32::NAN);
    valid_sibling.set_tiling_size(0.25);

    resize_tiling_container(&resized.clone().into(), 0.5);

    let sizes = [
      resized.tiling_size(),
      invalid_sibling.tiling_size(),
      valid_sibling.tiling_size(),
    ];
    assert!(sizes.iter().all(|size| size.is_finite()));
    assert!((sizes.iter().sum::<f32>() - 1.0).abs() < f32::EPSILON);

    drop(workspace);
  }
}
