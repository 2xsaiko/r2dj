use std::borrow::Borrow;
use std::fmt::{Display, Formatter};
use std::num::ParseIntError;
use std::ops::{
    Deref, Index, Range, RangeFrom, RangeFull, RangeInclusive, RangeTo, RangeToInclusive,
};
use std::str::FromStr;

#[derive(Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Default, Hash)]
pub struct TreePathBuf {
    path: Vec<u32>,
}

#[derive(Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[repr(transparent)]
pub struct TreePath {
    path: [u32],
}

impl TreePathBuf {
    pub const fn root() -> Self {
        TreePathBuf { path: Vec::new() }
    }

    pub fn push_index(&mut self, index: u32) {
        self.path.push(index);
    }

    pub fn extend_from(&mut self, other: impl AsRef<TreePath>) {
        self.path.extend_from_slice(other.as_ref().to_slice());
    }

    pub fn pop_index(&mut self) -> Option<u32> {
        self.path.pop()
    }

    pub fn increment_last(&mut self) {
        if let Some(s) = self.path.last_mut() {
            *s += 1;
        } else {
            self.path.push(0);
        }
    }
}

impl Deref for TreePathBuf {
    type Target = TreePath;

    fn deref(&self) -> &Self::Target {
        TreePath::new(&self.path)
    }
}

impl Borrow<TreePath> for TreePathBuf {
    fn borrow(&self) -> &TreePath {
        &**self
    }
}

impl Display for TreePathBuf {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        (**self).fmt(f)
    }
}

impl From<&[u32]> for TreePathBuf {
    fn from(path: &[u32]) -> Self {
        TreePathBuf {
            path: path.to_vec(),
        }
    }
}

impl AsRef<TreePath> for TreePathBuf {
    fn as_ref(&self) -> &TreePath {
        &**self
    }
}

impl AsRef<TreePath> for [u32] {
    fn as_ref(&self) -> &TreePath {
        TreePath::new(self)
    }
}

impl<const LEN: usize> AsRef<TreePath> for [u32; LEN] {
    fn as_ref(&self) -> &TreePath {
        TreePath::new(self)
    }
}

impl TreePath {
    pub fn new(idxs: &[u32]) -> &Self {
        unsafe { &*(idxs as *const [u32] as *const TreePath) }
    }

    pub fn strip_prefix<'a>(&'a self, prefix: &TreePath) -> Option<&'a TreePath> {
        if self.path.len() >= prefix.path.len()
            && prefix
                .path
                .iter()
                .zip(self.path.iter())
                .all(|(&a, &b)| a == b)
        {
            Some(TreePath::new(&self.path[prefix.path.len()..]))
        } else {
            None
        }
    }

    pub fn join(&self, other: impl AsRef<TreePath>) -> TreePathBuf {
        let mut new_path = self.to_owned();
        new_path.extend_from(other);
        new_path
    }

    pub fn to_tree_path_buf(&self) -> TreePathBuf {
        self.to_owned()
    }

    pub fn len(&self) -> usize {
        self.path.len()
    }

    pub fn is_empty(&self) -> bool {
        self.path.is_empty()
    }

    pub fn to_slice(&self) -> &[u32] {
        &self.path
    }
}

impl ToOwned for TreePath {
    type Owned = TreePathBuf;

    fn to_owned(&self) -> Self::Owned {
        self.path.into()
    }
}

impl Display for TreePath {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some((first, more)) = self.path.split_first() {
            write!(f, "{}", first)?;

            for el in more {
                write!(f, "-{}", el)?;
            }

            Ok(())
        } else {
            write!(f, "-")
        }
    }
}

impl FromStr for TreePathBuf {
    type Err = ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "" || s == "-" {
            Ok(TreePathBuf::root())
        } else {
            let mut path = Vec::new();

            for entry in s.split('-') {
                if entry.is_empty() {
                    continue;
                }

                let i = entry.parse()?;
                path.push(i);
            }

            Ok(TreePathBuf { path })
        }
    }
}

impl AsRef<TreePath> for TreePath {
    fn as_ref(&self) -> &TreePath {
        self
    }
}

macro_rules! impl_index {
    ($index_type:ty) => {
        impl Index<$index_type> for TreePath {
            type Output = TreePath;

            fn index(&self, index: $index_type) -> &Self::Output {
                TreePath::new(self.path.index(index))
            }
        }
    };
}

// haha
impl_index!(Range<usize>);
impl_index!(RangeFrom<usize>);
impl_index!(RangeFull);
impl_index!(RangeInclusive<usize>);
impl_index!(RangeTo<usize>);
impl_index!(RangeToInclusive<usize>);
