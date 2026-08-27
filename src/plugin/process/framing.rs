use super::ProcessError;
use crate::plugin::protocol::{Envelope, MAX_PROTOCOL_LINE_BYTES, decode_line};
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

pub async fn read_envelope<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Envelope, ProcessError> {
    let line = read_bounded_line(reader).await?;
    if line.is_empty() {
        return Err(ProcessError::UnexpectedEof);
    }
    decode_line(&line).map_err(|_| ProcessError::MalformedProtocol)
}

async fn read_bounded_line<R: AsyncBufRead + Unpin>(reader: &mut R) -> Result<Vec<u8>, ProcessError> {
    let mut line = Vec::new();
    loop {
        let buffer = reader.fill_buf().await.map_err(|_| ProcessError::Io)?;
        if buffer.is_empty() {
            return Ok(line);
        }
        let consumed = buffer
            .iter()
            .position(|byte| *byte == b'\n')
            .map_or(buffer.len(), |index| index + 1);
        if line.len() + consumed > MAX_PROTOCOL_LINE_BYTES + 1 {
            return Err(ProcessError::OversizedMessage);
        }
        line.extend_from_slice(&buffer[..consumed]);
        reader.consume(consumed);
        if line.last() == Some(&b'\n') {
            return Ok(line);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::BufReader;

    #[tokio::test]
    async fn bounds_input_before_json_parsing() {
        let bytes = vec![b'x'; MAX_PROTOCOL_LINE_BYTES + 2];
        let mut reader = BufReader::new(bytes.as_slice());
        assert_eq!(read_envelope(&mut reader).await, Err(ProcessError::OversizedMessage));
    }
}
