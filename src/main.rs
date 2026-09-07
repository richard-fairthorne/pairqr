//! ADB QR Code Pairing Tool
//!
//! Generates a QR code that your Android device can scan to pair for wireless debugging.
//! Based on: https://gist.github.com/benigumocom/a6a87fc1cb690c3c4e3a7642ebf2be6f

use mdns_sd::{ServiceDaemon, ServiceEvent};
use qrcode::QrCode;
use rand::Rng;
use std::net::IpAddr;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// mDNS service type for ADB pairing
const PAIRING_SERVICE: &str = "_adb-tls-pairing._tcp.local.";

/// mDNS service type for ADB connect (TLS)
const CONNECT_SERVICE: &str = "_adb-tls-connect._tcp.local.";

/// mDNS service type for legacy ADB connect
const CONNECT_SERVICE_LEGACY: &str = "_adb._tcp.local.";

/// Standard 24-byte ADB A_CNXN header + 7-byte banner payload "host::\0"
const ADB_CNXN_PACKET: &[u8] = b"CNXN\x00\x00\x00\x01\x00\x10\x00\x00\x07\x00\x00\x00\x32\x02\x00\x00\xbc\xb1\xa7\xb1host::\x00";

/// Characters for random name generation
const NAME_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";

/// Characters for random password generation
const PASS_CHARS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789!@#$%";

/// Generate a random string from the given character set
fn random_string(chars: &[u8], length: usize) -> String {
    let mut rng = rand::thread_rng();
    (0..length)
        .map(|_| {
            let idx = rng.gen_range(0..chars.len());
            chars[idx] as char
        })
        .collect()
}

/// Display QR code in terminal using half-block Unicode characters
/// This packs 2 QR rows into 1 terminal row, producing a compact output like Python's qrcode library
fn display_qr_code(data: &str) {
    let code = match QrCode::new(data) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to generate QR code: {}", e);
            return;
        }
    };

    let colors = code.to_colors();
    let width = code.width();

    // Quiet zone (border) in modules
    let border = 2;

    // Helper to check if a module is dark (treating out-of-bounds as light for border)
    let is_dark = |row: isize, col: isize| -> bool {
        if row < 0 || col < 0 || row >= width as isize || col >= width as isize {
            false
        } else {
            colors[row as usize * width + col as usize] == qrcode::Color::Dark
        }
    };

    // Process 2 QR rows at a time using half-block characters
    // ▀ (upper half) = top dark, bottom light
    // ▄ (lower half) = top light, bottom dark
    // █ (full block) = both dark
    // ' ' (space) = both light
    let total_rows = width + border * 2;
    let total_cols = width + border * 2;

    for row_pair in (0..total_rows).step_by(2) {
        for col in 0..total_cols {
            let qr_row_top = row_pair as isize - border as isize;
            let qr_row_bot = qr_row_top + 1;
            let qr_col = col as isize - border as isize;

            let top_dark = is_dark(qr_row_top, qr_col);
            let bot_dark = is_dark(qr_row_bot, qr_col);

            // Invert: dark QR modules should appear dark (background), light modules should appear light (foreground)
            // On dark terminal: space = dark background, block = light foreground
            let ch = match (top_dark, bot_dark) {
                (true, true) => ' ',   // both dark → background
                (true, false) => '▄',  // top dark (bg), bottom light (fg) → lower half block
                (false, true) => '▀',  // top light (fg), bottom dark (bg) → upper half block
                (false, false) => '█', // both light → full block
            };
            print!("{}", ch);
        }
        println!();
    }
}

/// Run adb pair command - returns (success, Option<guid>)
fn adb_pair(ip: &str, port: u16, password: &str) -> (bool, Option<String>) {
    println!("\n[*] Running: adb pair {}:{} ******", ip, port);

    let output = Command::new("adb")
        .args(["pair", &format!("{}:{}", ip, port), password])
        .output();

    match output {
        Ok(result) => {
            let stdout = String::from_utf8_lossy(&result.stdout);
            let stderr = String::from_utf8_lossy(&result.stderr);

            // Print actual output for debugging
            if !stdout.trim().is_empty() {
                println!("    adb: {}", stdout.trim());
            }
            if !stderr.trim().is_empty() {
                println!("    adb err: {}", stderr.trim());
            }

            if stdout.contains("Successfully paired") {
                println!("[+] Pairing successful!");

                // Extract GUID from output: "Successfully paired to IP:PORT [guid=XXX]"
                let guid = stdout
                    .split("[guid=")
                    .nth(1)
                    .and_then(|s| s.split(']').next())
                    .map(|s| s.to_string());

                (true, guid)
            } else {
                println!("[-] Pairing may have failed");
                (false, None)
            }
        }
        Err(e) => {
            println!("[-] Failed to run adb: {}", e);
            (false, None)
        }
    }
}

/// Get the preferred IP address (IPv4 over IPv6) and format it for ADB
fn get_preferred_ip(addresses: &std::collections::HashSet<IpAddr>) -> Option<String> {
    let addresses: Vec<_> = addresses.iter().collect();
    let addr = addresses
        .iter()
        .find(|a| a.is_ipv4())
        .or(addresses.first())
        .copied()?;

    Some(if addr.is_ipv6() {
        format!("[{}]", addr)
    } else {
        addr.to_string()
    })
}

/// Helper to list attached device serials
fn get_connected_device_serials() -> Vec<String> {
    let mut serials = Vec::new();
    if let Ok(output) = Command::new("adb").args(["devices"]).output() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        for line in stdout.lines().skip(1) {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() >= 2 && parts[1] == "device" {
                serials.push(parts[0].to_string());
            }
        }
    }
    serials
}

/// Check if a given port on target IP is genuinely an active ADB daemon
async fn is_adb_port(ip_addr: std::net::IpAddr, port: u16, probe_timeout: Duration) -> bool {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpStream;
    use tokio::time::timeout;

    let socket_addr = std::net::SocketAddr::new(ip_addr, port);
    let Ok(Ok(mut stream)) = timeout(probe_timeout, TcpStream::connect(socket_addr)).await else {
        return false;
    };

    if timeout(probe_timeout, stream.write_all(ADB_CNXN_PACKET)).await.is_err() {
        return false;
    }

    let mut buf = [0u8; 4];
    match timeout(probe_timeout, stream.read_exact(&mut buf)).await {
        Ok(Ok(_)) => &buf == b"STLS" || &buf == b"AUTH" || &buf == b"CNXN",
        _ => false,
    }
}

/// Fast scan for active ADB ports in 30000..=65535 on target IP.
/// Verifies each open socket with an ADB protocol handshake and exits early upon first match.
async fn scan_open_ports(ip: &str, exclude_port: Option<u16>) -> Vec<u16> {
    let ip_addr: std::net::IpAddr = match ip.parse() {
        Ok(addr) => addr,
        Err(_) => return Vec::new(),
    };

    let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(500));
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let cancel = std::sync::Arc::new(AtomicBool::new(false));

    for port in 30000..=65535u16 {
        if Some(port) == exclude_port {
            continue;
        }
        let sem = sem.clone();
        let tx = tx.clone();
        let cancel = cancel.clone();

        tokio::spawn(async move {
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let Ok(_permit) = sem.acquire().await else { return; };
            if cancel.load(Ordering::Relaxed) {
                return;
            }

            if is_adb_port(ip_addr, port, Duration::from_millis(300)).await {
                cancel.store(true, Ordering::Relaxed);
                let _ = tx.send(port).await;
            }
        });
    }

    drop(tx);

    let mut open_ports = Vec::new();
    while let Some(port) = rx.recv().await {
        open_ports.push(port);
        break;
    }
    open_ports
}

/// Show connected devices
fn show_devices() {
    println!("\n[*] Connected devices:");
    let _ = Command::new("adb").args(["devices", "-l"]).status();
}

#[tokio::main]
async fn main() {
    // Generate random credentials (like Android Studio does)
    let name = format!("studio-{}", random_string(NAME_CHARS, 10));
    let password = random_string(PASS_CHARS, 10);

    // QR code format (same as Android Studio)
    let qr_text = format!("WIFI:T:ADB;S:{};P:{};;", name, password);

    println!("{}", "=".repeat(50));
    println!("  pairqr v{} - ADB Wireless Debugging", env!("CARGO_PKG_VERSION"));
    println!("{}", "=".repeat(50));
    println!();

    // Display QR code
    display_qr_code(&qr_text);

    println!();
    println!("On your Android device:");
    println!("  1. Settings > Developer Options > Wireless Debugging");
    println!("  2. Tap 'Pair device with QR code'");
    println!("  3. Scan the QR code above");
    println!();
    println!("[*] Waiting for device to scan QR code...");
    println!("    (Press Ctrl+C to exit)");
    println!();

    // Track devices attached before pairing
    let initial_devices = get_connected_device_serials();

    // Set up Ctrl+C handler
    let running = Arc::new(AtomicBool::new(true));
    let r = running.clone();

    ctrlc::set_handler(move || {
        r.store(false, Ordering::SeqCst);
    })
    .expect("Error setting Ctrl-C handler");

    // Create mDNS service daemon
    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("Failed to create mDNS daemon: {}", e);
            return;
        }
    };

    // Browse for pairing service
    let pairing_receiver = match mdns.browse(PAIRING_SERVICE) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Failed to browse for pairing services: {}", e);
            return;
        }
    };

    let mut paired = false;
    let mut device_guid: Option<String> = None;
    let mut device_ip: Option<String> = None;
    let mut pairing_port: Option<u16> = None;

    // Wait for pairing
    while running.load(Ordering::SeqCst) && !paired {
        match pairing_receiver.recv_timeout(Duration::from_millis(100)) {
            Ok(event) => match event {
                ServiceEvent::ServiceResolved(info) => {
                    // Only process pairing services matching our generated QR code service name
                    // Ignore unrelated pairing broadcasts (e.g. adb-<guid> from 6-digit PIN pairing or other devices)
                    if !info.get_fullname().contains(&name) {
                        continue;
                    }

                    println!("\n[+] Device found: {}", info.get_fullname());

                    if let Some(ip) = get_preferred_ip(info.get_addresses()) {
                        let port = info.get_port();

                        println!("    Server: {}", info.get_hostname());
                        println!("    Port: {}", port);

                        let (success, guid) = adb_pair(&ip, port, &password);
                        if success {
                            paired = true;
                            device_guid = guid;
                            device_ip = Some(ip);
                            pairing_port = Some(port);
                        }
                    }
                }
                ServiceEvent::ServiceRemoved(_type, service_name) => {
                    if service_name.contains(&name) {
                        println!("\n[!] Service removed: {}", service_name);
                    }
                }
                _ => {}
            },
            Err(flume::RecvTimeoutError::Timeout) => continue,
            Err(flume::RecvTimeoutError::Disconnected) => break,
        }
    }

    if !running.load(Ordering::SeqCst) {
        println!("\n\n[*] Cancelled by user");
        let _ = mdns.shutdown();
        return;
    }

    if !paired {
        let _ = mdns.shutdown();
        return;
    }

    println!("\n[*] Looking for device connect service...");

    // Start browsing for connect services AFTER pairing completes so mDNS sends a fresh query
    let connect_receiver = mdns.browse(CONNECT_SERVICE).ok();
    let legacy_receiver = mdns.browse(CONNECT_SERVICE_LEGACY).ok();

    let mut connected = false;
    let mut last_scan = std::time::Instant::now() - Duration::from_secs(10);
    let timeout = std::time::Instant::now();

    while running.load(Ordering::SeqCst) && !connected && timeout.elapsed() < Duration::from_secs(15) {
        // 1. Check if ADB daemon auto-connected to a new endpoint matching target IP or GUID
        let current_devices = get_connected_device_serials();
        for dev in &current_devices {
            if !initial_devices.contains(dev) {
                let clean_guid = device_guid.as_ref().map(|g| g.trim_start_matches("adb-").to_string());
                let matches_ip = device_ip.as_ref().map_or(false, |ip| dev.starts_with(ip));
                let matches_guid = device_guid.as_ref().map_or(false, |guid| dev.contains(guid))
                    || clean_guid.as_ref().map_or(false, |cg| dev.contains(cg));

                if matches_ip || matches_guid {
                    connected = true;
                    println!("[+] Auto-connected by ADB: {}", dev);
                    break;
                }
            }
        }

        if connected {
            break;
        }

        // Helper to process resolved mDNS service events
        let handle_mdns_event = |event: ServiceEvent| -> bool {
            if let ServiceEvent::ServiceResolved(info) = event {
                let clean_guid = device_guid.as_ref().map(|g| g.trim_start_matches("adb-").to_string());
                let matches_guid = device_guid.as_ref().map_or(false, |guid| {
                    info.get_fullname().contains(guid) || info.get_hostname().contains(guid)
                }) || clean_guid.as_ref().map_or(false, |cg| {
                    info.get_fullname().contains(cg) || info.get_hostname().contains(cg)
                });

                let matches_ip = device_ip.as_ref().map_or(false, |ip| {
                    info.get_addresses().iter().any(|a| {
                        let a_str = a.to_string();
                        a_str == *ip || format!("[{}]", a_str) == *ip
                    })
                });

                if matches_guid || matches_ip {
                    if let Some(ip) = get_preferred_ip(info.get_addresses()) {
                        let port = info.get_port();
                        let addr = format!("{}:{}", ip, port);
                        println!("[+] Connect service found matching target (mDNS): {}", addr);

                        if let Ok(result) = Command::new("adb").args(["connect", &addr]).output() {
                            let out = String::from_utf8_lossy(&result.stdout);
                            println!("    adb: {}", out.trim());
                            if (out.contains("connected to") || out.contains("already connected to"))
                                && !out.contains("failed")
                            {
                                println!("[+] Connected!");
                                return true;
                            }
                        }
                    }
                }
            }
            false
        };

        // 2. Check mDNS events for connect service matching target device
        if let Some(ref rx) = connect_receiver {
            while let Ok(event) = rx.recv_timeout(Duration::from_millis(50)) {
                if handle_mdns_event(event) {
                    connected = true;
                    break;
                }
            }
        }

        if !connected {
            if let Some(ref rx) = legacy_receiver {
                while let Ok(event) = rx.recv_timeout(Duration::from_millis(50)) {
                    if handle_mdns_event(event) {
                        connected = true;
                        break;
                    }
                }
            }
        }

        if connected {
            break;
        }

        // 3. Fallback: Fast ADB TCP port scan starting after 2 seconds, retried periodically every 3 seconds
        if !connected && timeout.elapsed() >= Duration::from_secs(2) && last_scan.elapsed() >= Duration::from_secs(3) {
            last_scan = std::time::Instant::now();
            if let Some(ref ip) = device_ip {
                println!("[*] Scanning active wireless debugging ports on {}...", ip);
                let open_ports = scan_open_ports(ip, pairing_port).await;
                if open_ports.is_empty() {
                    println!("[-] No active ADB connect ports found on {} yet, retrying...", ip);
                } else {
                    for p in open_ports {
                        let addr = format!("{}:{}", ip, p);
                        println!("[*] Discovered active ADB wireless debugging port: {}", addr);
                        println!("[*] Connecting to {}...", addr);
                        if let Ok(result) = Command::new("adb").args(["connect", &addr]).output() {
                            let out = String::from_utf8_lossy(&result.stdout);
                            println!("    adb: {}", out.trim());
                            if (out.contains("connected to") || out.contains("already connected to"))
                                && !out.contains("failed")
                            {
                                println!("[+] Connected!");
                                connected = true;
                                break;
                            }
                        }
                    }
                }
            }
        }

        if !connected {
            tokio::time::sleep(Duration::from_millis(150)).await;
        }
    }

    // Cleanup
    let _ = mdns.shutdown();

    if !connected {
        // Prompt for manual port entry
        if let Some(ref ip) = device_ip {
            println!("[-] Could not auto-discover connect service for {}.", ip);
            println!("    Your device IP is: {}", ip);
            println!();
            println!("    On your device, go to Wireless Debugging settings");
            println!("    and look for the port number under 'IP address & Port' (e.g., 44061).");
            println!();
            print!("    Enter the port number (or press Enter to skip): ");
            use std::io::{self, Write};
            let _ = io::stdout().flush();

            let mut input = String::new();
            if io::stdin().read_line(&mut input).is_ok() {
                let input = input.trim();
                if !input.is_empty() {
                    if let Ok(port) = input.parse::<u16>() {
                        let addr = format!("{}:{}", ip, port);
                        println!("[*] Connecting to {}...", addr);

                        if let Ok(result) = Command::new("adb").args(["connect", &addr]).output() {
                            let out = String::from_utf8_lossy(&result.stdout);
                            println!("    adb: {}", out.trim());
                            if out.contains("connected") || out.contains("already") {
                                println!("[+] Connected!");
                                connected = true;
                            }
                        }
                    } else {
                        if input.len() == 6 && input.chars().all(|c| c.is_ascii_digit()) {
                            println!("    Note: '{}' is a 6-digit pairing code. Port numbers are 5 digits from 'IP address & Port' (e.g. 44061).", input);
                        } else {
                            println!("    Invalid port number");
                        }
                    }
                }
            }
        }

        if !connected {
            println!("[-] Not connected. Run manually: adb connect <ip>:<port>");
        }
    }

    show_devices();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    static TEST_PORT_COUNTER: std::sync::atomic::AtomicU16 = std::sync::atomic::AtomicU16::new(0);

    async fn bind_isolated_port() -> (TcpListener, u16) {
        let base = 18100 + TEST_PORT_COUNTER.fetch_add(20, Ordering::SeqCst);
        for port in base..base + 20 {
            if let Ok(l) = TcpListener::bind(format!("127.0.0.1:{}", port)).await {
                return (l, port);
            }
        }
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = l.local_addr().unwrap().port();
        (l, port)
    }

    #[tokio::test]
    async fn test_is_adb_port_stls_handshake() {
        let (listener, port) = bind_isolated_port().await;
        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 31];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(b"STLS\x01\x00\x00\x00").await;
            }
        });

        assert!(is_adb_port(ip, port, Duration::from_millis(500)).await);
    }

    #[tokio::test]
    async fn test_is_adb_port_auth_handshake() {
        let (listener, port) = bind_isolated_port().await;
        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 31];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(b"AUTH\x01\x00\x00\x00").await;
            }
        });

        assert!(is_adb_port(ip, port, Duration::from_millis(500)).await);
    }

    #[tokio::test]
    async fn test_is_adb_port_rejects_non_adb() {
        let (listener, port) = bind_isolated_port().await;
        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let _ = socket.write_all(b"HTTP/1.1 400 Bad Request\r\n\r\n").await;
            }
        });

        assert!(!is_adb_port(ip, port, Duration::from_millis(500)).await);
    }

    #[tokio::test]
    async fn test_is_adb_port_closed_port() {
        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        let port = {
            let (listener, p) = bind_isolated_port().await;
            drop(listener);
            p
        };

        assert!(!is_adb_port(ip, port, Duration::from_millis(300)).await);
    }

    #[tokio::test]
    async fn test_scan_open_ports_finds_mock_adb() {
        let (listener, port) = {
            let mut bound = None;
            for p in 30010..=31000 {
                if let Ok(l) = TcpListener::bind(format!("127.0.0.1:{}", p)).await {
                    bound = Some((l, p));
                    break;
                }
            }
            match bound {
                Some(pair) => pair,
                None => return,
            }
        };

        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buf = [0u8; 31];
                let _ = socket.read(&mut buf).await;
                let _ = socket.write_all(b"STLS\x01\x00\x00\x00").await;
            }
        });

        let found = scan_open_ports("127.0.0.1", None).await;
        assert_eq!(found, vec![port]);
    }

    #[test]
    fn test_random_string() {
        let s1 = random_string(NAME_CHARS, 10);
        let s2 = random_string(NAME_CHARS, 10);
        assert_eq!(s1.len(), 10);
        assert_eq!(s2.len(), 10);
        assert_ne!(s1, s2);
        assert!(s1.chars().all(|c| NAME_CHARS.contains(&(c as u8))));
    }

    #[test]
    fn test_get_preferred_ip_prefers_ipv4() {
        let mut addrs = std::collections::HashSet::new();
        let ipv4: IpAddr = "192.168.1.50".parse().unwrap();
        let ipv6: IpAddr = "fe80::1".parse().unwrap();
        addrs.insert(ipv6);
        addrs.insert(ipv4);

        let preferred = get_preferred_ip(&addrs);
        assert_eq!(preferred, Some("192.168.1.50".to_string()));
    }

    #[test]
    fn test_get_preferred_ip_formats_ipv6() {
        let mut addrs = std::collections::HashSet::new();
        let ipv6: IpAddr = "2001:db8::1".parse().unwrap();
        addrs.insert(ipv6);

        let preferred = get_preferred_ip(&addrs);
        assert_eq!(preferred, Some("[2001:db8::1]".to_string()));
    }

    #[test]
    fn test_get_preferred_ip_empty() {
        let addrs = std::collections::HashSet::new();
        assert_eq!(get_preferred_ip(&addrs), None);
    }

    #[test]
    fn test_display_qr_code_no_panic() {
        display_qr_code("WIFI:T:ADB;S:studio-test;P:password;;");
    }
}

