pub(crate) trait SetOrExpected<T: PartialEq> {
    /// If the current value is empty, it is set to `val` and `Ok` is returned. If the current value is
    /// not empty, it is compared against `val`, and if they are not equal `Err(current)` is returned.
    fn set_or_expected(&mut self, val: T) -> Result<(), &T>;
}

impl<T: PartialEq> SetOrExpected<T> for Option<T> {
    fn set_or_expected(&mut self, val: T) -> Result<(), &T> {
        match self {
            Some(cur) => (*cur == val).then_some(()).ok_or(cur),
            None => {
                *self = Some(val);
                Ok(())
            }
        }
    }
}
