pub use crate::core::{AudioSource, Core, OutputSignal};

pub mod core;
pub mod extra;
pub mod streamio;

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
