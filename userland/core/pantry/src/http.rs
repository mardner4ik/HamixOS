use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;
use hamix_std::{net, print, sys};

const MAX_REDIRECTS: usize = 5;
const RECV_TIMEOUT_MS: u64 = 20_000;
const MAX_BODY: usize = 512 * 1024 * 1024;

struct Url {
    host: String,
    port: u16,
    path: String,
}

fn parse_url(url: &str) -> Result<Url, String> {
    let rest = url.strip_prefix("http://").ok_or_else(|| format!("unsupported URL (only http:// works for now): {}", url))?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().map_err(|_| format!("bad port in {}", url))?),
        None => (authority, 80),
    };
    if host.is_empty() {
        return Err(format!("bad URL {}", url));
    }
    Ok(Url { host: host.to_string(), port, path: path.to_string() })
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn human(bytes: usize) -> String {
    if bytes >= 1 << 20 {
        format!("{}.{} MiB", bytes >> 20, ((bytes % (1 << 20)) * 10) >> 20)
    } else {
        format!("{} KiB", bytes.div_ceil(1024))
    }
}

fn dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(body.len());
    let mut pos = 0usize;
    loop {
        let line_end = find(&body[pos..], b"\r\n").ok_or("truncated chunked body")? + pos;
        let size_text = core::str::from_utf8(&body[pos..line_end]).map_err(|_| "bad chunk header")?;
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16).map_err(|_| "bad chunk size")?;
        pos = line_end + 2;
        if size == 0 {
            return Ok(out);
        }
        let chunk = body.get(pos..pos + size).ok_or("truncated chunk")?;
        out.extend_from_slice(chunk);
        pos += size + 2;
    }
}

fn fetch_once(url: &Url, label: &str, quiet: bool) -> Result<(u32, Vec<(String, String)>, Vec<u8>), String> {
    let ip = net::resolve(&url.host).map_err(|e| format!("cannot resolve {}: {}", url.host, sys::error_name(e)))?;
    let mut stream = net::TcpStream::connect(ip, url.port, 15_000).map_err(|e| format!("cannot connect to {}:{}: {}", url.host, url.port, sys::error_name(e)))?;
    let request = format!("GET {} HTTP/1.1\r\nHost: {}\r\nUser-Agent: pantry/0.1 (HamixOS)\r\nAccept: */*\r\nConnection: close\r\n\r\n", url.path, url.host);
    if stream.send(request.as_bytes()) < 0 {
        return Err(format!("sending request to {} failed", url.host));
    }
    let mut data: Vec<u8> = Vec::new();
    let mut buf = alloc::vec![0u8; 64 * 1024];
    let mut header_end: Option<usize> = None;
    let mut expected: Option<usize> = None;
    let mut last_shown = 0usize;
    loop {
        let n = stream.recv(&mut buf, RECV_TIMEOUT_MS);
        if n < 0 {
            return Err(format!("download from {} failed: {}", url.host, sys::error_name(n)));
        }
        if n == 0 {
            break;
        }
        data.extend_from_slice(&buf[..n as usize]);
        if data.len() > MAX_BODY {
            return Err(String::from("response too large"));
        }
        if header_end.is_none() {
            if let Some(end) = find(&data, b"\r\n\r\n") {
                header_end = Some(end + 4);
                let head = String::from_utf8_lossy(&data[..end]).to_ascii_lowercase();
                expected = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok());
                if let Some(total) = expected {
                    if total <= MAX_BODY {
                        data.reserve((end + 4 + total).saturating_sub(data.len()));
                    }
                }
            }
        }
        if let (Some(start), false) = (header_end, quiet) {
            let got = data.len() - start;
            if got - last_shown >= 256 * 1024 {
                last_shown = got;
                match expected {
                    Some(total) if total > 0 => print!("\r  {} {} / {} ({}%)   ", label, human(got), human(total), got * 100 / total),
                    _ => print!("\r  {} {}   ", label, human(got)),
                }
            }
        }
        if let (Some(start), Some(total)) = (header_end, expected) {
            if data.len() - start >= total {
                break;
            }
        }
    }
    if last_shown > 0 {
        print!("\r\x1b[K");
    }
    let start = header_end.ok_or_else(|| format!("malformed response from {}", url.host))?;
    let head = String::from_utf8_lossy(&data[..start]).into_owned();
    let mut lines = head.lines();
    let status_line = lines.next().unwrap_or("");
    let status: u32 = status_line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).ok_or_else(|| format!("bad status line: {}", status_line))?;
    let headers: Vec<(String, String)> = lines.filter_map(|l| l.split_once(':')).map(|(k, v)| (k.trim().to_ascii_lowercase(), v.trim().to_string())).collect();
    data.drain(..start);
    let mut body = data;
    if headers.iter().any(|(k, v)| k == "transfer-encoding" && v.to_ascii_lowercase().contains("chunked")) {
        body = dechunk(&body)?;
    } else if let Some(total) = expected {
        if body.len() < total {
            return Err(format!("connection closed early ({} of {} bytes)", body.len(), total));
        }
        body.truncate(total);
    }
    Ok((status, headers, body))
}

pub fn get(url: &str, label: &str, quiet: bool) -> Result<Vec<u8>, String> {
    let mut current = String::from(url);
    for _ in 0..MAX_REDIRECTS {
        let parsed = parse_url(&current)?;
        let (status, headers, body) = fetch_once(&parsed, label, quiet)?;
        match status {
            200 => return Ok(body),
            301 | 302 | 303 | 307 | 308 => {
                let location = headers.iter().find(|(k, _)| k == "location").map(|(_, v)| v.clone()).ok_or("redirect without Location")?;
                current = if location.starts_with('/') { format!("http://{}:{}{}", parsed.host, parsed.port, location) } else { location };
            }
            404 => return Err(format!("not found: {}", current)),
            other => return Err(format!("server answered {} for {}", other, current)),
        }
    }
    Err(format!("too many redirects for {}", url))
}
