use std::ptr::{self, NonNull};

pub use objc2_application_services::{AXError, AXUIElement};
use objc2_core_foundation::{CFRetained, CFString, CFType};

use crate::Error;

/// Extension trait for [`AXUIElement`].
pub trait AXUIElementExt {
  /// Retrieves the value of an accessibility attribute.
  ///
  /// # Errors
  ///
  /// Returns an error if:
  /// - The accessibility operation fails (e.g. invalid attribute name).
  /// - The attribute value cannot be cast to the requested type.
  fn get_attribute<T: objc2_core_foundation::Type>(
    &self,
    attribute: &str,
  ) -> crate::Result<CFRetained<T>>;

  /// Sets the value of an accessibility attribute.
  ///
  /// # Errors
  ///
  /// Returns an error if the accessibility operation fails.
  fn set_attribute<T: objc2_core_foundation::Type + AsRef<CFType>>(
    &self,
    attribute: &str,
    value: &CFRetained<T>,
  ) -> crate::Result<()>;
}

impl AXUIElementExt for AXUIElement {
  fn get_attribute<T: objc2_core_foundation::Type>(
    &self,
    attribute: &str,
  ) -> crate::Result<CFRetained<T>> {
    let mut value: *const CFType = ptr::null();

    // SAFETY: `self` is a live `AXUIElement`, and the out-parameter
    // points to the `value` local, which outlives the call.
    let result = unsafe {
      self.copy_attribute_value(
        &CFString::from_str(attribute),
        // SAFETY: Stack address of `value` is guaranteed to be
        // non-null.
        NonNull::new(&raw mut value).unwrap(),
      )
    };

    if result != AXError::Success {
      return Err(Error::Accessibility(attribute.to_string(), result.0));
    }

    NonNull::new(value.cast_mut())
      // SAFETY: `copy_attribute_value` follows the `Copy` rule, so it
      // returns a +1 reference that `from_raw` takes ownership of without
      // an extra retain. The caller picks `T`, which must match the
      // attribute's real type.
      .map(|ptr| unsafe { CFRetained::from_raw(ptr.cast()) })
      .ok_or_else(|| {
        Error::InvalidPointer(
          "copy_attribute_value returned success but null pointer"
            .to_string(),
        )
      })
  }

  fn set_attribute<T: objc2_core_foundation::Type + AsRef<CFType>>(
    &self,
    attribute: &str,
    value: &CFRetained<T>,
  ) -> crate::Result<()> {
    let cf_attribute = CFString::from_str(attribute);
    // SAFETY: `self` is a live `AXUIElement`, and both the attribute name
    // and the value are owned `CFType`s that outlive the call.
    let result =
      unsafe { self.set_attribute_value(&cf_attribute, value.as_ref()) };

    if result != AXError::Success {
      return Err(Error::Accessibility(attribute.to_string(), result.0));
    }

    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use objc2_core_foundation::CFString;

  use super::*;

  #[test]
  fn get_attribute_invalid_attribute_is_err() {
    let pid = i32::try_from(std::process::id()).expect("pid overflow");

    // SAFETY: `new_application` accepts any PID and follows the `Create`
    // rule, so the returned element is owned and released on drop.
    let el = unsafe { AXUIElement::new_application(pid) };
    let result =
      el.get_attribute::<CFString>("AXDefinitelyNotARealAttribute");

    assert!(result.is_err());
  }

  #[test]
  fn set_attribute_invalid_attribute_is_err() {
    let pid = i32::try_from(std::process::id()).expect("pid overflow");

    // SAFETY: `new_application` accepts any PID and follows the `Create`
    // rule, so the returned element is owned and released on drop.
    let el = unsafe { AXUIElement::new_application(pid) };
    let value = CFString::from_str("dummy");
    let result = el.set_attribute("AXDefinitelyNotARealAttribute", &value);

    assert!(result.is_err());
  }
}
