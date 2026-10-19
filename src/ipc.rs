//! Tiny control socket: clients write one command word, the server reads it.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::{fs, thread};

pub fn socket_path() -> PathBuf {
    dirs::runtime_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("zunder.sock")
}

pub fn send(cmd: &str) -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path())?;
    stream.write_all(cmd.as_bytes())
}

/// Bind the socket and forward every received command into `tx`.
pub fn listen(tx: async_channel::Sender<String>) -> io::Result<()> {
    let path = socket_path();
    if UnixStream::connect(&path).is_ok() {
        return Err(io::Error::new(io::ErrorKind::AddrInUse, "server already running"));
    }
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    thread::spawn(move || {
        for mut stream in listener.incoming().flatten() {
            let mut buf = String::new();
            if stream.read_to_string(&mut buf).is_ok() && tx.send_blocking(buf.trim_end_matches('\n').to_owned()).is_err() {
                break;
            }
        }
    });
    Ok(())
}
