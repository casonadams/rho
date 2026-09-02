use crate::error::{AppError, Result};
use serde::{Serialize, de::DeserializeOwned};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub struct JsonLinesReader<R> {
    reader: R,
}

impl<R: AsyncBufRead + Unpin> JsonLinesReader<R> {
    pub fn new(reader: R) -> Self {
        Self { reader }
    }

    pub async fn read_message<T: DeserializeOwned>(&mut self) -> Result<Option<T>> {
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = self.reader.read_line(&mut line).await?;
            if bytes_read == 0 {
                return Ok(None);
            }
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            let msg = serde_json::from_str::<T>(trimmed)
                .map_err(|e| AppError::Provider(format!("JSON parsing error: {e}")))?;
            return Ok(Some(msg));
        }
    }
}

pub struct JsonLinesWriter<W> {
    writer: W,
}

impl<W: AsyncWrite + Unpin> JsonLinesWriter<W> {
    pub fn new(writer: W) -> Self {
        Self { writer }
    }

    pub async fn write_message<T: Serialize>(&mut self, message: &T) -> Result<()> {
        let mut json =
            serde_json::to_vec(message).map_err(|e| AppError::Provider(format!("JSON serialization error: {e}")))?;
        json.push(b'\n');
        self.writer.write_all(&json).await?;
        self.writer.flush().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::protocol::{RpcCommand, RpcEvent, RpcRequest};
    use std::io::Cursor;

    #[tokio::test]
    async fn test_json_lines_reader_and_writer() {
        let input_data =
            b"{\"id\":\"1\",\"type\":\"prompt\",\"message\":\"hi\"}\n\n{\"id\":\"2\",\"type\":\"abort\"}\n";
        let mut reader = JsonLinesReader::new(Cursor::new(input_data));

        let msg1: RpcRequest = reader.read_message().await.unwrap().unwrap();
        assert_eq!(msg1.id, Some("1".to_string()));
        assert!(matches!(msg1.command, RpcCommand::Prompt { .. }));

        let msg2: RpcRequest = reader.read_message().await.unwrap().unwrap();
        assert_eq!(msg2.id, Some("2".to_string()));
        assert!(matches!(msg2.command, RpcCommand::Abort));

        let msg3: Option<RpcRequest> = reader.read_message().await.unwrap();
        assert!(msg3.is_none());

        let mut output_buf = Vec::new();
        let mut writer = JsonLinesWriter::new(&mut output_buf);
        let event = RpcEvent::TextChunk {
            content: "output".to_string(),
        };
        writer.write_message(&event).await.unwrap();
        assert_eq!(output_buf, b"{\"type\":\"text_chunk\",\"content\":\"output\"}\n");
    }
}
