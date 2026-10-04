// Copyright (c) 2019 Ant Financial
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::error::{sock_error_msg, Error, Result};
use crate::proto::{check_oversize, MessageHeader, DEFAULT_PAGE_SIZE, MESSAGE_HEADER_LENGTH};
use crate::sync::sys::PipeConnection;

fn read_count(conn: &PipeConnection, count: usize) -> Result<Vec<u8>> {
    let mut v: Vec<u8> = vec![0; count];
    let len = read_into(conn, &mut v)?;
    v.truncate(len);
    Ok(v)
}

fn read_into(conn: &PipeConnection, buf: &mut [u8]) -> Result<usize> {
    let mut len = 0;
    while len < buf.len() {
        match conn.read(&mut buf[len..]) {
            Ok(l) => {
                len += l;
                // when socket peer closed, it would return 0.
                if l == 0 {
                    break;
                }
            }
            Err(e) => {
                return Err(Error::Socket(e.to_string()));
            }
        }
    }

    Ok(len)
}

fn discard_count(conn: &PipeConnection, count: usize) -> Result<()> {
    let mut need_discard = count;

    while need_discard > 0 {
        let once_discard = std::cmp::min(DEFAULT_PAGE_SIZE, need_discard);
        read_count(conn, once_discard)?;
        need_discard -= once_discard;
    }

    Ok(())
}

fn read_message_header(conn: &PipeConnection) -> Result<MessageHeader> {
    let mut buf = [0; MESSAGE_HEADER_LENGTH];
    let size = read_into(conn, &mut buf)?;
    if size != MESSAGE_HEADER_LENGTH {
        return Err(sock_error_msg(
            size,
            format!("Message header length {size} is too small"),
        ));
    }

    let mh = MessageHeader::from(buf);

    Ok(mh)
}

pub fn read_message(conn: &PipeConnection) -> Result<(MessageHeader, Result<Vec<u8>>)> {
    let mh = read_message_header(conn)?;
    trace!("Got Message header {:?}", mh);

    let mh_len = mh.length as usize;
    if let Err(e) = check_oversize(mh_len, true) {
        discard_count(conn, mh_len)?;
        return Ok((mh, Err(e)));
    }

    let buf = read_count(conn, mh.length as usize)?;
    let size = buf.len();
    if size != mh.length as usize {
        return Err(sock_error_msg(
            size,
            format!("Message length {} is not {}", size, mh.length),
        ));
    }
    trace!("Got Message body {:?}", buf);

    Ok((mh, Ok(buf)))
}

pub fn write_message(
    conn: &PipeConnection,
    header: MessageHeader,
    payload: &[u8],
    prefix: &mut Vec<u8>,
) -> Result<()> {
    write_frame(header, payload, prefix, |bytes| {
        conn.write(bytes).map_err(|e| Error::Socket(e.to_string()))
    })
}

fn write_frame(
    header: MessageHeader,
    payload: &[u8],
    prefix: &mut Vec<u8>,
    mut write: impl FnMut(&[u8]) -> Result<usize>,
) -> Result<()> {
    // Match the async writer: coalesce only a bounded prefix and reuse it.
    let prefix_len = payload.len().min(DEFAULT_PAGE_SIZE - MESSAGE_HEADER_LENGTH);
    let len = MESSAGE_HEADER_LENGTH + prefix_len;
    if prefix.capacity() < len {
        prefix.reserve_exact(len - prefix.len());
    }
    prefix.resize(len, 0);
    header.into_buf(&mut *prefix);
    prefix[MESSAGE_HEADER_LENGTH..].copy_from_slice(&payload[..prefix_len]);

    for mut remaining in [prefix.as_slice(), &payload[prefix_len..]] {
        while !remaining.is_empty() {
            let written = write(remaining)?;
            if written == 0 {
                return Err(Error::Socket(
                    "failed to write ttrpc frame: write zero".into(),
                ));
            }
            remaining = &remaining[written..];
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame_bytes(header: MessageHeader, payload: &[u8]) -> Vec<u8> {
        let mut bytes = vec![0; MESSAGE_HEADER_LENGTH];
        header.into_buf(&mut bytes);
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn writes_a_bounded_prefix_and_handles_short_writes() {
        let boundary = DEFAULT_PAGE_SIZE - MESSAGE_HEADER_LENGTH;
        for size in [0, 1, boundary - 1, boundary, boundary + 1, 128 * 1024] {
            let payload: Vec<_> = (0..size).map(|i| i as u8).collect();
            let header = MessageHeader::new_request(7, size as u32);
            for max_write in [usize::MAX, 3] {
                let mut prefix = Vec::new();
                let mut bytes = Vec::new();
                let mut writes = Vec::new();
                write_frame(header, &payload, &mut prefix, |buf| {
                    let n = buf.len().min(max_write);
                    writes.push(n);
                    bytes.extend_from_slice(&buf[..n]);
                    Ok(n)
                })
                .unwrap();
                assert_eq!(bytes, frame_bytes(header, &payload));
                assert!(prefix.len() <= DEFAULT_PAGE_SIZE);
                if max_write == usize::MAX {
                    assert_eq!(writes.len(), if size > boundary { 2 } else { 1 });
                    assert_eq!(
                        writes[0],
                        (size + MESSAGE_HEADER_LENGTH).min(DEFAULT_PAGE_SIZE)
                    );
                }
            }
        }
    }

    #[test]
    fn reuses_buffer_without_sending_stale_bytes() {
        let mut prefix = Vec::with_capacity(DEFAULT_PAGE_SIZE);
        let original_buffer = prefix.as_ptr();
        let original_capacity = prefix.capacity();
        let mut bytes = Vec::new();
        let mut expected = Vec::new();
        for (id, size) in [DEFAULT_PAGE_SIZE * 2, 0, 64, DEFAULT_PAGE_SIZE, 1]
            .iter()
            .copied()
            .enumerate()
        {
            let payload = vec![id as u8; size];
            let header = MessageHeader::new_response(id as u32, size as u32);
            write_frame(header, &payload, &mut prefix, |buf| {
                let n = buf.len().min(7);
                bytes.extend_from_slice(&buf[..n]);
                Ok(n)
            })
            .unwrap();
            assert_eq!(prefix.as_ptr(), original_buffer);
            assert_eq!(prefix.capacity(), original_capacity);
            expected.extend(frame_bytes(header, &payload));
        }
        assert_eq!(bytes, expected);
    }

    #[test]
    fn propagates_write_errors_and_rejects_zero_writes() {
        let payload = vec![1; DEFAULT_PAGE_SIZE];
        let header = MessageHeader::new_response(7, payload.len() as u32);
        // Fail in either the coalesced prefix or the remaining payload.
        for fail_at in [0, 1] {
            for zero in [false, true] {
                let mut calls = 0;
                let result = write_frame(header, &payload, &mut Vec::new(), |buf| {
                    let call = calls;
                    calls += 1;
                    if call != fail_at {
                        return Ok(buf.len());
                    }
                    if zero {
                        Ok(0)
                    } else {
                        Err(Error::Socket("broken pipe".into()))
                    }
                });
                assert!(matches!(result, Err(Error::Socket(_))));
                assert_eq!(calls, fail_at + 1);
            }
        }
    }

    #[cfg(unix)]
    #[test]
    fn round_trips_reused_frames_over_a_socket() {
        use std::os::unix::{io::AsRawFd, net::UnixStream};
        use std::{thread, time::Duration};

        let (sender, receiver) = UnixStream::pair().unwrap();
        sender
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        receiver
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let sizes = [128 * 1024, 1, 0, DEFAULT_PAGE_SIZE, 64];
        let reader = thread::spawn(move || {
            let conn = PipeConnection::new(receiver.as_raw_fd());
            for (id, size) in sizes.iter().copied().enumerate() {
                let (header, payload) = read_message(&conn).unwrap();
                assert_eq!(header, MessageHeader::new_response(id as u32, size as u32));
                assert_eq!(payload.unwrap(), vec![id as u8; size]);
            }
            assert!(read_message(&conn).is_err());
        });
        let conn = PipeConnection::new(sender.as_raw_fd());
        let mut prefix = Vec::new();
        for (id, size) in sizes.iter().copied().enumerate() {
            let header = MessageHeader::new_response(id as u32, size as u32);
            write_message(&conn, header, &vec![id as u8; size], &mut prefix).unwrap();
        }
        drop(sender);
        reader.join().unwrap();
    }
}
