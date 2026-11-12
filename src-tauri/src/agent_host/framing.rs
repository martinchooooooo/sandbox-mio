//! Byte-exact framing of an attach stream into journal records with their offsets.

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

use super::types::AttachEvent;

/// One raw line of the stream: its parsed record, or `None` when the line is not JSON.
pub struct Framed {
    pub event: Option<AttachEvent>,
}

/// Reads the next line from `reader` and advances `offset` by its raw length, terminator included.
/// Returns `None` at end of stream; a trailing partial line (no line feed) is dropped, because the
/// next reattach serves it again from the unadvanced offset.
// Splits on line feed only: pi's JSON may carry U+2028 or a lone `\r` inside strings.
pub async fn next_record<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    offset: &mut u64,
) -> std::io::Result<Option<Framed>> {
    let mut line = Vec::new();
    let n = reader.read_until(b'\n', &mut line).await?;
    if n == 0 || line.last() != Some(&b'\n') {
        return Ok(None);
    }
    let start = *offset;
    *offset += n as u64;
    let event = serde_json::from_slice::<serde_json::Value>(&line).ok().map(|record| AttachEvent {
        offset: Some(start),
        end: Some(*offset),
        record,
    });
    Ok(Some(Framed { event }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const DROP: &str = include_str!("fixtures/02a-drop.jsonl");
    const REATTACH: &str = include_str!("fixtures/02b-reattach.jsonl");

    async fn frame_all(bytes: &[u8], mut offset: u64) -> Vec<AttachEvent> {
        let mut reader = bytes;
        let mut out = Vec::new();
        while let Some(f) = next_record(&mut reader, &mut offset).await.unwrap() {
            out.extend(f.event);
        }
        out
    }

    fn journal() -> Vec<u8> {
        let mut j = DROP.replace("\r\n", "\n").into_bytes();
        j.extend(REATTACH.replace("\r\n", "\n").into_bytes());
        // A record whose string carries the separators other line readers split on.
        j.extend("{\"type\":\"x\",\"s\":\"a\u{2028}b\\r\\nc\u{2029}\"}\n".as_bytes());
        j
    }

    #[tokio::test]
    async fn offsets_are_contiguous_and_span_each_raw_line() {
        let j = journal();
        let events = frame_all(&j, 0).await;
        assert_eq!(events.len(), j.iter().filter(|b| **b == b'\n').count());
        let mut expected = 0;
        for e in &events {
            assert_eq!(e.offset, Some(expected));
            let end = e.end.unwrap() as usize;
            assert_eq!(j[end - 1], b'\n');
            expected = end as u64;
        }
        assert_eq!(expected, j.len() as u64);
    }

    #[tokio::test]
    async fn a_reattach_from_a_stored_cursor_resumes_without_gap_or_duplicate() {
        let j = journal();
        let whole = frame_all(&j, 0).await;
        // Drop the connection after 15 records and resume from the cursor the client stored.
        let cursor = whole[14].end.unwrap();
        let mut resumed = whole[..15].to_vec();
        resumed.extend(frame_all(&j[cursor as usize..], cursor).await);
        assert_eq!(resumed, whole);
    }

    #[tokio::test]
    async fn a_partial_trailing_line_is_not_consumed() {
        let mut offset = 0;
        let mut reader: &[u8] = b"{\"type\":\"a\"}\n{\"type\":\"b\"";
        assert!(next_record(&mut reader, &mut offset).await.unwrap().is_some());
        assert!(next_record(&mut reader, &mut offset).await.unwrap().is_none());
        assert_eq!(offset, 13);
    }

    #[tokio::test]
    async fn the_joined_text_deltas_equal_the_final_text() {
        let events = frame_all(&journal(), 0).await;
        let mut joined = String::new();
        let mut last_end = None;
        for e in &events {
            let ev = &e.record["assistantMessageEvent"];
            match ev["type"].as_str() {
                Some("text_delta") => joined.push_str(ev["delta"].as_str().unwrap()),
                Some("text_end") => last_end = ev["content"].as_str().map(str::to_string),
                _ => {}
            }
        }
        assert_eq!(Some(joined), last_end);
    }
}
