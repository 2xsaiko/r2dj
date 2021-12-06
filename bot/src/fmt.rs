use std::fmt::{Display, Formatter};

pub trait HtmlDisplay {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result;
}

impl<T> HtmlDisplay for &T
where
    T: HtmlDisplay,
{
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        (*self).fmt(f)
    }
}

pub trait HtmlDisplayExt: HtmlDisplay {
    fn html(&self) -> HtmlFormat<Self> {
        HtmlFormat { inner: self }
    }
}

impl<T> HtmlDisplayExt for T where T: HtmlDisplay {}

pub struct HtmlFormat<'a, T: ?Sized> {
    inner: &'a T,
}

impl<'a, T: ?Sized> Display for HtmlFormat<'a, T>
where
    T: HtmlDisplay,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.inner.fmt(f)
    }
}
