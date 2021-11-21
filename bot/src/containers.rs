use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A transparent copy-on-write smart pointer.
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash)]
pub struct Container<T> {
    inner: Arc<T>,
}

impl<T> Container<T> {
    pub fn wrap(t: T) -> Self {
        Container { inner: Arc::new(t) }
    }

    pub fn from_arc(arc: Arc<T>) -> Self {
        Container { inner: arc }
    }

    pub fn into_arc(self) -> Arc<T> {
        self.inner
    }

    pub fn to_arc(&self) -> &Arc<T> {
        &self.inner
    }
}

impl<T: Clone> Container<T> {
    pub fn into_inner(self) -> T {
        match Arc::try_unwrap(self.inner) {
            Ok(v) => v,
            Err(arc) => (*arc).clone(),
        }
    }
}

impl<T> Deref for Container<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &*self.inner
    }
}

impl<T: Clone> DerefMut for Container<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        Arc::make_mut(&mut self.inner)
    }
}

impl<T: Default> Default for Container<T> {
    fn default() -> Self {
        Container::wrap(Default::default())
    }
}

impl<T: Display> Display for Container<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

impl<T: Debug> Debug for Container<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}

#[macro_export]
macro_rules! declare_container {
    (
        $vis:vis type $cname:ident = Container<$iname:ty>;
    ) => {
        $vis type $cname = $iname;
    };
    (
        $vis:vis type $cname:ident = Container<$iname:ty> {
            $(
                $fv:vis fn $fname:ident( $($par:ident: $pty:ty),* $(,)? ) -> Self;
            )*
        }
    ) => {
        $vis type $cname = $crate::containers::Container<$iname>;

        impl $cname {
            $(
                $fv fn $fname( $($par: $pty),* ) -> Self {
                    $crate::containers::Container::wrap(<$iname>::$fname( $($par),* ))
                }
            )*
        }
    };
}

declare_container! {
    pub type LString = Container<String> {
        pub fn new() -> Self;
    }
}

pub type LVec<T> = Container<Vec<T>>;
pub type LHashMap<K, V> = Container<HashMap<K, V>>;

impl From<&str> for LString {
    fn from(s: &str) -> Self {
        LString::wrap(s.to_string())
    }
}

impl<T> LVec<T> {
    pub fn new() -> Self {
        LVec::wrap(Vec::new())
    }
}

impl<K, V> LHashMap<K, V> {
    pub fn new() -> Self {
        LHashMap::wrap(HashMap::new())
    }
}

#[cfg(test)]
mod test {
    use crate::containers::LString;

    #[test]
    fn test() {
        let mut s: LString = "a".into();
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
