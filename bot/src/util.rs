use std::io;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

pub async fn connect<I, O>(mut input: I, mut output: O) -> io::Result<()>
where
    I: AsyncRead + Unpin,
    O: AsyncWrite + Unpin,
{
    let mut buf = [0; 4096];

    loop {
        let len = input.read(&mut buf).await?;

        if len == 0 {
            break;
        }

        output.write_all(&buf[..len]).await?;
    }

    output.shutdown().await?;

    Ok(())
}

pub(crate) fn slice_to_u8(slice: &[i16]) -> &[u8] {
    unsafe { std::slice::from_raw_parts(slice.as_ptr() as *const u8, slice.len() * 2) }
}

pub(crate) fn slice_to_u8_mut(slice: &mut [i16]) -> &mut [u8] {
    unsafe { std::slice::from_raw_parts_mut(slice.as_mut_ptr() as *mut u8, slice.len() * 2) }
}

