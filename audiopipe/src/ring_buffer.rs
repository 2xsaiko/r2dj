use dasp::ring_buffer::SliceMut;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;

pub trait BufferRead<S> {
    fn pop() -> Option<S>;
}

pub trait BufferWrite<S>
where S: SliceMut {
    fn push(element: S::Element) -> Option<S::Element>;
}

pub struct BoundedRead<S> {
    head: AtomicUsize,
    tail: AtomicUsize,
    data: Arc<S>,
}

pub struct BoundedWrite<S> {
    head: AtomicUsize,
    tail: AtomicUsize,
    data: Arc<S>,
}

pub struct Bounded<S> {
    head: usize,
    tail: usize,
    data: S,
}

impl <S> Bounded<S> {
    pub fn split(self) -> (BoundedRead<S>, BoundedWrite<S>) {
        unimplemented!()
    }
}