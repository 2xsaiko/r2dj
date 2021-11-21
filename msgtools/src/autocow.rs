use std::fmt::{Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A transparent copy-on-write smart pointer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Ac<T: ?Sized> {
    inner: Arc<T>,
}

impl<T> Ac<T> {
    pub fn new(t: T) -> Self {
        Ac { inner: Arc::new(t) }
    }
}

impl<T: ?Sized> Ac<T> {
    pub fn from_arc(arc: Arc<T>) -> Self {
        Ac { inner: arc }
    }

    pub fn into_arc(self) -> Arc<T> {
        self.inner
    }

    pub fn to_arc(&self) -> &Arc<T> {
        &self.inner
    }
}

impl<T: Clone> Ac<T> {
    pub fn into_inner(self) -> T {
        match Arc::try_unwrap(self.inner) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

impl<T: ?Sized> Deref for Ac<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<T: Clone + ?Sized> DerefMut for Ac<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.inner)
    }
}

impl<T: Default> Default for Ac<T> {
    fn default() -> Self {
        Ac::new(Default::default())
    }
}

impl<T: Display + ?Sized> Display for Ac<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Debug + ?Sized> Debug for Ac<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T> From<T> for Ac<T> {
    fn from(s: T) -> Self {
        Ac::new(s)
    }
}

impl From<&str> for Ac<String> {
    fn from(s: &str) -> Self {
        Ac::new(s.to_string())
    }
}

#[cfg(test)]
mod test {
    use super::Ac;

    #[test]
    fn test() {
        let mut s: Ac<String> = "a".into();
        let ptr1 = s.to_arc().as_ptr();
        s.push_str("bc");
        assert_eq!(ptr1, s.to_arc().as_ptr());

        let mut copy = s.clone();
        assert_eq!(s.to_arc().as_ptr(), copy.to_arc().as_ptr());

        copy.push_str("def");
        assert_ne!(s.to_arc().as_ptr(), copy.to_arc().as_ptr());

        assert_eq!("abc", &*s);
        assert_eq!("abcdef", &*copy);
    }
}
