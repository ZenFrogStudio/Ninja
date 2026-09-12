use anyhow::Context;

use super::{distribute_tiling_size, flatten_split_container};
use crate::{models::Container, traits::CommonGetters};

/// Removes a container from the tree.
///
/// If the container is a tiling container, the siblings will be resized to
/// fill the freed up space. Will flatten empty parent split containers.
#[allow(clippy::needless_pass_by_value)]
pub fn detach_container(child_to_remove: Container) -> anyhow::Result<()> {
  // Flatten the parent split container if it'll be empty after removing
  // the child.
  if let Some(split_parent) = child_to_remove
    .parent()
    .and_then(|parent| parent.as_split().cloned())
  {
    if split_parent.child_count() == 1 {
      flatten_split_container(split_parent)?;
    }
  }

  let parent = child_to_remove.parent().context("No parent.")?;

  parent
    .borrow_children_mut()
    .retain(|c| c.id() != child_to_remove.id());

  parent
    .borrow_child_focus_order_mut()
    .retain(|id| *id != child_to_remove.id());

  *child_to_remove.borrow_parent_mut() = None;

  // Resize the siblings if it is a tiling container.
  if child_to_remove.as_tiling_container().is_ok() {
    let tiling_siblings = parent.tiling_children().collect::<Vec<_>>();
    distribute_tiling_size(&tiling_siblings, 1.0);
  }

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::detach_container;
  use crate::{
    models::{TilingWindow, Workspace},
    traits::{TilingSizeGetters, MIN_TILING_SIZE},
  };

  #[test]
  fn fills_freed_space_when_all_siblings_are_at_minimum() {
    let removed = TilingWindow::mock().call();
    let sibling_a = TilingWindow::mock().call();
    let sibling_b = TilingWindow::mock().call();
    let workspace = Workspace::mock()
      .tiling_containers(vec![
        removed.clone().into(),
        sibling_a.clone().into(),
        sibling_b.clone().into(),
      ])
      .call();

    removed.set_tiling_size(1.0 - (2.0 * MIN_TILING_SIZE));
    sibling_a.set_tiling_size(MIN_TILING_SIZE);
    sibling_b.set_tiling_size(MIN_TILING_SIZE);

    detach_container(removed.into()).unwrap();

    assert!(sibling_a.tiling_size().is_finite());
    assert!(sibling_b.tiling_size().is_finite());
    assert!((sibling_a.tiling_size() - 0.5).abs() < f32::EPSILON);
    assert!((sibling_b.tiling_size() - 0.5).abs() < f32::EPSILON);

    drop(workspace);
  }
}
