//! worker_fixture：E03a 受控故障 fixture（独立 bin，仅 test-fixtures 构建）。
//! 只做合成管道 IO：不读剪贴板、不写数据库、不启动 WebView。
//! stdout 只输出协议帧；stdin 读请求帧；stderr 静默。

use std::io::{Read, Write};
use std::process;
use std::thread;
use std::time::Duration;

use pinfeather_lib::clipboard::protocol::{
    encode_response_frame, parse_frame_prefix, parse_worker_message_json, WorkerMessage,
    FRAME_LENGTH_BYTES,
};
use pinfeather_lib::contracts::WORKER_REQUEST_MAX_BYTES;

fn read_exact_stdin(buf: &mut [u8]) -> Option<usize> {
    let mut filled = 0usize;
    while filled < buf.len() {
        let mut handle = std::io::stdin().lock();
        match handle.read(&mut buf[filled..]) {
            Ok(0) => return Some(filled),
            Ok(n) => filled += n,
            Err(_) => return None,
        }
    }
    Some(filled)
}

fn read_request() -> Option<WorkerMessage> {
    let mut prefix = [0u8; FRAME_LENGTH_BYTES];
    match read_exact_stdin(&mut prefix)? {
        0 => return None,
        n if n < FRAME_LENGTH_BYTES => return None,
        _ => {}
    }
    let declared = parse_frame_prefix(prefix, WORKER_REQUEST_MAX_BYTES).ok()?;
    let mut body = vec![0u8; declared];
    match read_exact_stdin(&mut body)? {
        n if n < declared => return None,
        _ => {}
    }
    let text = std::str::from_utf8(&body).ok()?;
    parse_worker_message_json(text).ok()
}

fn write_frame(body: &[u8]) {
    let frame = encode_response_frame(body).expect("fixture frame within limit");
    let mut out = std::io::stdout().lock();
    let _ = out.write_all(&frame);
    let _ = out.flush();
}

fn ack_body(request: &WorkerMessage, capture_id: &str, policy_epoch: i64) -> String {
    format!(
        "{{\"version\":{},\"capture_id\":\"{}\",\"policy_epoch\":{}}}",
        request.version, capture_id, policy_epoch
    )
}

fn block_until_stdin_eof() {
    let mut sink = [0u8; 64];
    let mut handle = std::io::stdin().lock();
    while matches!(handle.read(&mut sink), Ok(n) if n > 0) {}
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .and_then(|a| a.strip_prefix("--mode=").map(String::from))
        .unwrap_or_else(|| String::from("good"));

    // 握手阶段（除 no-hello 系）：输出严格 hello。
    match mode.as_str() {
        "no-hello" => {
            block_until_stdin_eof();
            return;
        }
        "bad-hello" => {
            write_frame(b"{\"kind\":\"hi\",\"version\":1}");
            block_until_stdin_eof();
            return;
        }
        "bad-hello-version" => {
            write_frame(b"{\"kind\":\"hello\",\"version\":2}");
            block_until_stdin_eof();
            return;
        }
        "hello-extra" => {
            write_frame(b"{\"kind\":\"hello\",\"version\":1,\"extra\":true}");
            block_until_stdin_eof();
            return;
        }
        "never-read" => {
            write_frame(br#"{"kind":"hello","version":1}"#);
            // 完全不读 stdin：制造父端写入积压与无响应路径。
            loop {
                thread::park();
            }
        }
        "flood-idle" => {
            write_frame(br#"{"kind":"hello","version":1}"#);
            // 空闲期洪泛：未经请求的控制帧连续涌入。
            for index in 0..8 {
                let body = format!(
                    "{{\"version\":1,\"capture_id\":\"FLOOD{index}\",\"policy_epoch\":1}}"
                );
                write_frame(body.as_bytes());
            }
            block_until_stdin_eof();
            return;
        }
        _ => write_frame(br#"{"kind":"hello","version":1}"#),
    }

    loop {
        let request = match read_request() {
            Some(request) => request,
            None => return,
        };

        match mode.as_str() {
            "good" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
            }
            "no-response" => {
                block_until_stdin_eof();
                return;
            }
            "half-length" => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&[0x05, 0x00]);
                let _ = out.flush();
                return;
            }
            "half-body" => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&50u32.to_le_bytes());
                let _ = out.write_all(b"only-ten-by");
                let _ = out.flush();
                return;
            }
            "oversize" => {
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&(19u32 * 1024 * 1024).to_le_bytes());
                let _ = out.flush();
                return;
            }
            "bad-json" => {
                write_frame(b"not json");
                return;
            }
            "wrong-id" => {
                let body = ack_body(&request, "X-WRONG", request.policy_epoch);
                write_frame(body.as_bytes());
                return;
            }
            "wrong-epoch" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch + 1);
                write_frame(body.as_bytes());
                return;
            }
            "crash" => {
                process::abort();
            }
            "late" => {
                thread::sleep(Duration::from_millis(3000));
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                return;
            }
            "double-ack" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                write_frame(body.as_bytes());
            }
            "matching-ack-flood" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                for _ in 0..8 {
                    write_frame(body.as_bytes());
                }
                block_until_stdin_eof();
                return;
            }
            "ack-then-partial" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(&[5, 0]);
                let _ = out.flush();
                return;
            }
            "trickle" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                let frame = encode_response_frame(body.as_bytes()).unwrap();
                let mut out = std::io::stdout().lock();
                for byte in frame {
                    let _ = out.write_all(&[byte]);
                    let _ = out.flush();
                    thread::sleep(Duration::from_millis(5));
                }
            }
            "flood-before-ack" => {
                // 洪泛先于真实 ACK：不得被解释为成功。
                for index in 0..8 {
                    let body = format!(
                        "{{\"version\":1,\"capture_id\":\"FLOOD{index}\",\"policy_epoch\":1}}"
                    );
                    write_frame(body.as_bytes());
                }
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                return;
            }
            "dup-per-round" => {
                // 每轮先给 1 个已结束 ID 的重复帧（旧 ID）再给 ACK；每批至多 2 帧。
                if request.capture_id != "DUP-1" {
                    let stale = ack_body(&request, "DUP-1", 1);
                    write_frame(stale.as_bytes());
                }
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
            }
            "ack-pad-4096" => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                let pad = 4096usize - body.len();
                let mut padded = " ".repeat(pad);
                padded.push_str(&body);
                assert_eq!(padded.len(), 4096);
                write_frame(padded.as_bytes());
                return;
            }
            "ack-oversize" => {
                // 控制帧超限：正文 >4 KiB。
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                let mut padded = " ".repeat(4097 - body.len());
                padded.push_str(&body);
                assert!(padded.len() > 4096);
                write_frame(padded.as_bytes());
                return;
            }
            other if other.starts_with("delay-") => {
                let millis: u64 = other["delay-".len()..].parse().unwrap_or(0);
                thread::sleep(Duration::from_millis(millis));
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                return;
            }
            "driven" => {
                // 受控故障按 capture_id 前缀驱动，便于同一 supervisor 上编排多次捕获。
                let id = request.capture_id.as_str();
                if id.starts_with("STALL") {
                    block_until_stdin_eof();
                    return;
                } else if id.starts_with("CRASH") {
                    process::abort();
                } else if id.starts_with("LATE") {
                    thread::sleep(Duration::from_millis(3000));
                    let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                    write_frame(body.as_bytes());
                } else {
                    let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                    write_frame(body.as_bytes());
                }
            }
            _ => {
                let body = ack_body(&request, &request.capture_id, request.policy_epoch);
                write_frame(body.as_bytes());
                return;
            }
        }
    }
}
