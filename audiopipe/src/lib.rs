pub mod buffer;
pub mod mixer;
pub mod mixerv2;
pub mod streamio;
pub mod aaaaaaa;
pub mod ring_buffer;

fn slice_to_u8(slice: &[i16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 2) }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
